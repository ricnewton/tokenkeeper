//! Fake usage data for the `--popup-sheet` layout preview.

use crate::model::{Detail, Meter, ProviderId, Snapshot, Unit};
use crate::time::{self, DAY_MS};

const CYCLE_DAYS: usize = 30;
const DAYS_ELAPSED: f32 = 16.5;
const NOW_HOUR: usize = 14;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub fn simulated(used_fraction: f32) -> Snapshot {
    let limit = 20_000;
    let used = (used_fraction.max(0.0) * limit as f32) as u32;
    let mut rng = Lcg(7);
    let now = time::now_ms();
    let start = now - (DAYS_ELAPSED * DAY_MS as f32) as i64;

    let days_done = DAYS_ELAPSED.ceil() as usize;
    let shape: Vec<f32> = (0..CYCLE_DAYS)
        .map(|i| {
            let weekend = i % 7 == 0 || i % 7 == 6;
            (if weekend { 0.15 } else { 1.0 }) * (0.4 + rng.next() * 1.2)
        })
        .collect();
    let shape_sum: f32 = shape[..days_done].iter().sum();
    let mut daily: Vec<Option<u32>> = shape
        .iter()
        .enumerate()
        .map(|(i, v)| (i < days_done).then(|| (v / shape_sum * used as f32) as u32))
        .collect();
    if let Some(Some(today)) = daily.get_mut(days_done - 1) {
        *today /= 2;
    }
    let daily_labels = (0..CYCLE_DAYS as i64)
        .map(|i| {
            let t = time::local(start + i * DAY_MS);
            format!("{}/{}", t.wDay, t.wMonth)
        })
        .collect();

    let today = daily[days_done - 1].unwrap_or(0);
    let hour_shape: Vec<f32> = (0..24)
        .map(|h| {
            if (8..=18).contains(&h) {
                0.5 + rng.next()
            } else {
                rng.next() * 0.08
            }
        })
        .collect();
    let hour_sum: f32 = hour_shape[..=NOW_HOUR].iter().sum();
    let hourly = hour_shape
        .iter()
        .enumerate()
        .map(|(h, v)| (h <= NOW_HOUR).then(|| (v / hour_sum * today as f32) as u32))
        .collect();

    let history = [0.71, 0.88, 1.12, 0.64, 0.93]
        .iter()
        .map(|f| (f * limit as f32) as u32)
        .chain(std::iter::once(used))
        .collect();
    let history_labels = (0..6)
        .rev()
        .map(|i| time::month(start - i * 30 * DAY_MS).to_string())
        .collect();

    let mut s = Snapshot::new(ProviderId::Cursor, Unit::Cents);
    s.plan = "Enterprise".into();
    s.limit = limit;
    s.used = used;
    s.cycle_start_ms = start;
    s.cycle_end_ms = start + CYCLE_DAYS as i64 * DAY_MS;
    s.fetched_ms = now;
    s.today = Some(today);
    s.daily = daily;
    s.daily_labels = daily_labels;
    s.hourly = hourly;
    s.history = history;
    s.history_labels = history_labels;
    s
}

/// The Cursor mock at half the allowance.
pub fn simulated_copilot(used_fraction: f32) -> Snapshot {
    let mut s = simulated(used_fraction);
    let scale = |v: u32| v / 2;
    s.provider = ProviderId::Copilot;
    s.limit = scale(s.limit);
    s.used = scale(s.used);
    s.today = s.today.map(scale);
    s.daily = s.daily.iter().map(|v| v.map(scale)).collect();
    s.hourly = s.hourly.iter().map(|v| v.map(scale)).collect();
    s.history = s.history.iter().map(|v| scale(*v)).collect();
    s
}

/// A weekly rate-limit window with a 5-hour session and a per-model limit.
pub fn simulated_claude(used_fraction: f32) -> Snapshot {
    let now = time::now_ms();
    let mut s = Snapshot::new(ProviderId::Claude, Unit::Basis);
    s.plan = "Enterprise".into();
    s.limit = 10_000;
    s.used = (used_fraction * 10_000.0) as u32;
    s.cycle_start_ms = now - 3 * DAY_MS - 5 * 3_600_000;
    s.cycle_end_ms = s.cycle_start_ms + 7 * DAY_MS;
    s.meters = vec![
        Meter {
            label: "5-hour session".into(),
            fraction: 0.62,
            resets_ms: Some(now + 2 * 3_600_000 + 1_200_000),
        },
        Meter {
            label: "Weekly, all models".into(),
            fraction: used_fraction,
            resets_ms: Some(s.cycle_end_ms),
        },
        Meter {
            label: "Weekly, Opus".into(),
            fraction: 0.86,
            resets_ms: Some(s.cycle_end_ms),
        },
    ];
    s.details = vec![Detail::Limits, Detail::Cycle, Detail::History];
    let per_day = [900, 1400, 1300, 600];
    s.daily = (0..8).map(|i| per_day.get(i).map(|v| *v)).collect();
    s.daily_labels = (0..8)
        .map(|i| {
            let t = time::local(s.cycle_start_ms + i as i64 * DAY_MS);
            format!("{}/{}", t.wDay, t.wMonth)
        })
        .collect();
    s.history = vec![7100, 8800, 10_000, 6400, s.used];
    s.history_labels = (0..5)
        .rev()
        .map(|i| time::day_month(s.cycle_start_ms - i * 7 * DAY_MS))
        .collect();
    s
}

/// A spend-capped Claude seat: dollars against a monthly cap, no rate-limit windows.
#[cfg(windows)]
pub fn simulated_claude_spend(used_fraction: f32) -> Snapshot {
    let mut s = simulated(used_fraction);
    s.provider = ProviderId::Claude;
    s.limit = 60_000;
    s.used = (used_fraction * 60_000.0) as u32;
    s.today = None;
    s.hourly.clear();
    s.details = vec![Detail::Limits, Detail::Cycle, Detail::History];
    s.meters = vec![Meter {
        label: "Extra usage this month".into(),
        fraction: used_fraction,
        resets_ms: None,
    }];
    s
}

/// ChatGPT Codex subscription rate window in demo mode.
#[cfg(target_os = "linux")]
pub fn simulated_codex(used_fraction: f32) -> Snapshot {
    let now = time::now_ms();
    let mut s = Snapshot::new(ProviderId::Codex, Unit::Basis);
    s.plan = "Pro · Weekly".into();
    s.limit = 10_000;
    s.used = (used_fraction * 10_000.0) as u32;
    s.cycle_start_ms = now - 3 * DAY_MS;
    s.cycle_end_ms = s.cycle_start_ms + 7 * DAY_MS;
    s.meters = vec![Meter {
        label: "Weekly".into(),
        fraction: used_fraction,
        resets_ms: Some(s.cycle_end_ms),
    }];
    s.details = vec![Detail::Limits];
    s
}

pub fn simulated_opencode(used_fraction: f32) -> Snapshot {
    let now = time::now_ms();
    let mut s = Snapshot::new(ProviderId::OpenCode, Unit::Basis);
    s.plan = "Go · Monthly".into();
    s.limit = 10_000;
    s.used = (used_fraction * 10_000.0) as u32;
    s.cycle_end_ms = now + 18 * DAY_MS;
    s.meters = vec![
        Meter { label: "5-hour session".into(), fraction: 0.25, resets_ms: Some(now + 2 * 3_600_000) },
        Meter { label: "Weekly".into(), fraction: 0.46, resets_ms: Some(now + 2 * DAY_MS) },
        Meter { label: "Monthly".into(), fraction: used_fraction, resets_ms: Some(s.cycle_end_ms) },
    ];
    s.details = vec![Detail::Limits];
    s
}
