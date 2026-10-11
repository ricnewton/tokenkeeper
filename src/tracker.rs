//! Builds per-hour, per-day and per-cycle breakdowns from periodic readings
//! of a running total, for providers whose APIs only report the total so far.
//!
//! Usage is attributed to the hour in which a reading first saw it, so time
//! the PC was off lands in the next observed hour. Whatever was already used
//! at the first reading of a cycle is spread evenly over the earlier days.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::model::{Detail, Snapshot};
use crate::store;
use crate::time::{self, DAY_MS};

const HOUR_MS: i64 = 3_600_000;
const PAST_CYCLES: usize = 5;
/// Reset times that wobble by less than this are the same cycle.
const SAME_CYCLE_MS: i64 = 2 * HOUR_MS;

pub struct Tracker {
    file: &'static str,
    loaded: bool,
    cycle_start: i64,
    /// Local wall-clock hour index → last total seen in that hour.
    hours: BTreeMap<i64, u32>,
    /// Past cycle start → final total.
    past: BTreeMap<i64, u32>,
}

fn local_hour(ms: i64) -> i64 {
    time::local_wall_ms(ms).div_euclid(HOUR_MS)
}

impl Tracker {
    pub fn new(file: &'static str) -> Self {
        Tracker { file, loaded: false, cycle_start: 0, hours: BTreeMap::new(), past: BTreeMap::new() }
    }

    pub fn reset(&mut self) {
        *self = Tracker::new(self.file);
        self.loaded = true;
        store::save_json(self.file, &json!({}));
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(v) = store::load_json(self.file) else { return };
        let map = |v: &Value| -> BTreeMap<i64, u32> {
            v.as_object()
                .map(|m| m.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_u64()? as u32))).collect())
                .unwrap_or_default()
        };
        self.cycle_start = v["cycle_start"].as_i64().unwrap_or(0);
        self.hours = map(&v["hours"]);
        self.past = map(&v["past"]);
    }

    fn save(&self) {
        let map = |m: &BTreeMap<i64, u32>| -> Value {
            Value::Object(m.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>())
        };
        store::save_json(
            self.file,
            &json!({ "cycle_start": self.cycle_start, "hours": map(&self.hours), "past": map(&self.past) }),
        );
    }

    /// Records `s.used` as of now, then fills in the breakdown fields of `s`.
    pub fn record(&mut self, s: &mut Snapshot, label: impl Fn(i64) -> String) {
        self.load();
        if (s.cycle_start_ms - self.cycle_start).abs() > SAME_CYCLE_MS {
            if let Some(&last) = self.hours.values().next_back() {
                if self.cycle_start != 0 && s.cycle_start_ms > self.cycle_start {
                    self.past.insert(self.cycle_start, last);
                    while self.past.len() > PAST_CYCLES {
                        self.past.pop_first();
                    }
                }
            }
            self.hours.clear();
            self.cycle_start = s.cycle_start_ms;
        }
        self.hours.insert(local_hour(time::now_ms()), s.used);
        self.save();
        self.apply(s, label);
    }

    fn apply(&self, s: &mut Snapshot, label: impl Fn(i64) -> String) {
        let now = time::now_ms();
        let first_hour = local_hour(s.cycle_start_ms);
        let observed_from = self.hours.keys().next().copied().unwrap_or(i64::MAX);
        let day = |ms: i64| time::local_wall_ms(ms).div_euclid(DAY_MS);
        let (first, last, today) = (day(s.cycle_start_ms), day(s.cycle_end_ms - 1), day(now));
        let days = (last - first + 1).max(1);
        // Whole days of the cycle before the first reading.
        let unobserved = (observed_from.div_euclid(24) - first).clamp(0, days);

        // Usage first seen in each hour.
        let mut deltas: BTreeMap<i64, u32> = BTreeMap::new();
        let mut baseline = 0;
        let mut prev: Option<u32> = None;
        for (&h, &v) in &self.hours {
            let d = match prev {
                Some(p) => v.saturating_sub(p),
                // On the cycle's first day the first reading accounts for everything so far.
                None if h <= first_hour || unobserved == 0 => v,
                None => {
                    baseline = v;
                    0
                }
            };
            deltas.insert(h, d);
            prev = Some(v);
        }
        // Integer shares of `baseline` over the unobserved days that sum exactly.
        let share = |i: i64| (baseline as i64 * (i + 1) / unobserved - baseline as i64 * i / unobserved) as u32;

        s.daily = (first..first + days)
            .map(|d| {
                if d - first < unobserved {
                    return Some(share(d - first));
                }
                let observed = (d + 1) * 24 > observed_from && d <= today;
                observed.then(|| deltas.range(d * 24..(d + 1) * 24).map(|(_, v)| v).sum())
            })
            .collect();
        s.daily_labels = (0..days)
            .map(|i| {
                let t = time::local(s.cycle_start_ms + i * DAY_MS);
                format!("{}/{}", t.wDay, t.wMonth)
            })
            .collect();

        let now_hour = local_hour(now);
        let midnight = today * 24;
        s.hourly = (0..24)
            .map(|i| {
                let h = midnight + i;
                (h <= now_hour && h >= observed_from.min(now_hour)).then(|| deltas.get(&h).copied().unwrap_or(0))
            })
            .collect();
        if s.details.contains(&Detail::Today) {
            s.today = Some(deltas.range(midnight..midnight + 24).map(|(_, v)| v).sum());
        }

        let mut history: Vec<(i64, u32)> = self.past.iter().map(|(k, v)| (*k, *v)).collect();
        history.push((s.cycle_start_ms, s.used));
        s.history_labels = history.iter().map(|(start, _)| label(*start)).collect();
        s.history = history.into_iter().map(|(_, v)| v).collect();
        s.empty_note = "Collecting data, check back later";
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderId, Unit};

    /// First seen mid-cycle with usage already on the clock, as after a fresh sign-in.
    #[test]
    fn spreads_usage_before_first_reading() {
        let now = time::now_ms();
        let mut s = Snapshot::new(ProviderId::Copilot, Unit::Cents);
        s.cycle_start_ms = now - 5 * DAY_MS;
        s.cycle_end_ms = s.cycle_start_ms + 30 * DAY_MS;
        s.used = 5620;
        let mut t = Tracker::new("unused.json");
        t.cycle_start = s.cycle_start_ms;
        t.hours.insert(local_hour(now) - 1, 5600);
        t.hours.insert(local_hour(now), 5620);
        t.apply(&mut s, |_| String::new());

        let known: Vec<u32> = s.daily.iter().flatten().copied().collect();
        assert_eq!(known.iter().sum::<u32>(), 5620);
        assert!(known.len() >= 5 && known[0] > 1000, "{known:?}");
        assert_eq!(s.today, Some(20));
    }
}
