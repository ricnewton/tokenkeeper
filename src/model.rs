//! Provider-neutral usage data that the tray icon and popups render.

use std::rc::Rc;

use crate::time::{self, DAY_MS};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderId {
    Cursor,
    Copilot,
    Claude,
    Codex,
    OpenCode,
}

impl ProviderId {
    pub const ALL: [ProviderId; 5] = [
        ProviderId::Cursor,
        ProviderId::Copilot,
        ProviderId::Claude,
        ProviderId::Codex,
        ProviderId::OpenCode,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ProviderId::Cursor => "Cursor",
            ProviderId::Copilot => "GitHub Copilot",
            ProviderId::Claude => "Claude Code",
            ProviderId::Codex => "ChatGPT Codex",
            ProviderId::OpenCode => "OpenCode Go",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            ProviderId::Cursor => "Cursor",
            ProviderId::Copilot => "Copilot",
            ProviderId::Claude => "Claude",
            ProviderId::Codex => "Codex",
            ProviderId::OpenCode => "OpenCode",
        }
    }

    /// Web page with the provider's own (more detailed) usage view.
    pub fn usage_url(self) -> &'static str {
        match self {
            ProviderId::Cursor => "https://cursor.com/dashboard?tab=usage",
            ProviderId::Copilot => "https://github.com/settings/copilot/features",
            ProviderId::Claude => "https://claude.ai/settings/usage",
            ProviderId::Codex => "https://chatgpt.com/codex/settings/usage",
            ProviderId::OpenCode => "https://opencode.ai/console",
        }
    }

    /// Where the browser sign-in happens, for status text.
    #[cfg(windows)]
    pub fn sign_in_site(self) -> &'static str {
        match self {
            ProviderId::Cursor => "cursor.com",
            ProviderId::Copilot => "github.com",
            ProviderId::Claude => "claude.ai",
            ProviderId::Codex => "chatgpt.com",
            ProviderId::OpenCode => "opencode.ai",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Cents,
    /// Hundredths of a percent of a rate limit.
    Basis,
}

/// A detail view the panel can show as a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Today,
    Cycle,
    History,
    Limits,
}

impl Detail {
    pub fn label(self) -> &'static str {
        match self {
            Detail::Today => "Today",
            Detail::Cycle => "This cycle",
            Detail::History => "History",
            Detail::Limits => "Limits",
        }
    }
}

/// A secondary limit, e.g. Claude's 5-hour window.
#[derive(Clone, Debug)]
pub struct Meter {
    pub label: String,
    pub fraction: f32,
    pub resets_ms: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub provider: ProviderId,
    pub plan: String,
    pub unit: Unit,
    pub limit: u32,
    pub used: u32,
    pub cycle_start_ms: i64,
    pub cycle_end_ms: i64,
    pub fetched_ms: i64,
    pub today: Option<u32>,
    /// Usage per day of the cycle; `None` for days that haven't happened yet
    /// (or weren't observed). Empty until a breakdown is available.
    pub daily: Vec<Option<u32>>,
    pub daily_labels: Vec<String>,
    /// Usage per hour of today; `None` for future hours. Empty if unknown.
    pub hourly: Vec<Option<u32>>,
    /// Totals for past cycles, oldest first, ending with the current cycle.
    pub history: Vec<u32>,
    pub history_labels: Vec<String>,
    pub meters: Vec<Meter>,
    /// Panel tabs, in order.
    pub details: Vec<Detail>,
    /// Shown instead of "Breakdown not loaded yet" when a chart is empty.
    pub empty_note: &'static str,
}

impl Snapshot {
    pub fn new(provider: ProviderId, unit: Unit) -> Self {
        Snapshot {
            provider,
            plan: String::new(),
            unit,
            limit: 0,
            used: 0,
            cycle_start_ms: 0,
            cycle_end_ms: 0,
            fetched_ms: time::now_ms(),
            today: None,
            daily: Vec::new(),
            daily_labels: Vec::new(),
            hourly: Vec::new(),
            history: Vec::new(),
            history_labels: Vec::new(),
            meters: Vec::new(),
            details: vec![Detail::Today, Detail::Cycle, Detail::History],
            empty_note: "Breakdown not loaded yet",
        }
    }
}

/// What the UI should show for one provider right now.
#[derive(Clone)]
pub enum View {
    Loading,
    /// With the reason the last sign-in failed or the session ended, if any.
    SignedOut(Option<String>),
    /// `code` is a device code the user must enter in the browser.
    SigningIn {
        code: Option<String>,
    },
    /// A fetch failed before any data arrived.
    Failed(String),
    Ready {
        snap: Rc<Snapshot>,
        error: Option<String>,
    },
}

impl View {
    pub fn account_menu_label(&self, name: &str) -> String {
        match self {
            Self::Loading => format!("{name} — Loading…"),
            Self::SignedOut(_) => format!("{name} — Sign in…"),
            Self::SigningIn { .. } => format!("{name} — Cancel sign-in"),
            Self::Ready { .. } | Self::Failed(_) => format!("{name} — Sign out"),
        }
    }
    pub fn snapshot(&self) -> Option<&Snapshot> {
        match self {
            View::Ready { snap, .. } => Some(snap),
            _ => None,
        }
    }
}

impl Snapshot {
    pub fn used_fraction(&self) -> f32 {
        if self.limit == 0 {
            0.0
        } else {
            self.used as f32 / self.limit as f32
        }
    }

    pub fn cycle_days(&self) -> f32 {
        ((self.cycle_end_ms - self.cycle_start_ms) as f32 / DAY_MS as f32).max(1.0)
    }

    pub fn elapsed_fraction(&self) -> f32 {
        if self.cycle_start_ms <= 0 || self.cycle_end_ms <= self.cycle_start_ms {
            return 1.0;
        }
        let span = (self.cycle_end_ms - self.cycle_start_ms).max(1) as f32;
        ((time::now_ms() - self.cycle_start_ms) as f32 / span).clamp(0.0, 1.0)
    }

    /// Projected fraction used at cycle end if the current rate continues.
    pub fn projected_fraction(&self) -> f32 {
        self.used_fraction() / self.elapsed_fraction().max(0.02)
    }

    pub fn days_left(&self) -> u32 {
        ((self.cycle_end_ms - time::now_ms()).max(0) as f32 / DAY_MS as f32).ceil() as u32
    }

    pub fn pace_text(&self) -> String {
        if self.cycle_start_ms <= 0 || self.cycle_end_ms <= self.cycle_start_ms {
            return "Usage pace unavailable".into();
        }
        let p = (self.projected_fraction() * 100.0).round();
        let note = if p > 102.0 {
            " (over)"
        } else if p > 90.0 {
            " (tight)"
        } else {
            ""
        };
        format!("On pace for ~{p}%{note}")
    }

    pub fn fmt(&self, v: u32) -> String {
        match self.unit {
            Unit::Cents => dollars(v),
            Unit::Basis => format!("{}%", (v as f32 / 100.0).round()),
        }
    }

    /// The line under the headline figure, e.g. "of $2,500.00 included".
    pub fn of_text(&self) -> String {
        match self.unit {
            _ if self.limit == 0 => "used, no allowance reported".into(),
            Unit::Basis if self.cycle_start_ms <= 0 => "of the subscription limit".into(),
            Unit::Cents => format!("of {} included", self.fmt(self.limit)),
            Unit::Basis => format!("of the {}-day limit", self.cycle_days().round()),
        }
    }

    pub fn resets_text(&self) -> String {
        if self.cycle_end_ms <= 0 { return "Reset time not reported".into(); }
        let left = self.cycle_end_ms - time::now_ms();
        if left < DAY_MS {
            let hours = (left.max(0) as f32 / 3_600_000.0).ceil() as u32;
            let unit = if hours == 1 { "hour" } else { "hours" };
            return format!("Resets in {hours} {unit}");
        }
        let days = self.days_left();
        let unit = if days == 1 { "day" } else { "days" };
        format!(
            "Resets {} ({days} {unit})",
            time::day_month(self.cycle_end_ms)
        )
    }
}

pub fn dollars(cents: u32) -> String {
    format!("${}.{:02}", thousands(cents / 100), cents % 100)
}

fn thousands(v: u32) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Providers the tray icon can switch to: signed-out or empty ones would only
/// show a blank ring.
pub fn switch_targets<'a>(views: impl IntoIterator<Item = &'a View>) -> Vec<usize> {
    views
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| v.snapshot().map(|_| i))
        .collect()
}

/// The provider a wheel step lands on, cycling through `ready` only. Positive
/// `delta` (wheel up) moves to the previous one.
pub fn scroll_target(active: usize, delta: i32, ready: &[usize]) -> Option<usize> {
    if delta == 0 || ready.is_empty() {
        return None;
    }
    if let Some(position) = ready.iter().position(|&i| i == active) {
        let next =
            (position as isize - delta.signum() as isize).rem_euclid(ready.len() as isize) as usize;
        Some(ready[next])
    } else if delta > 0 {
        ready
            .iter()
            .rev()
            .copied()
            .find(|&i| i < active)
            .or_else(|| ready.last().copied())
    } else {
        ready
            .iter()
            .copied()
            .find(|&i| i > active)
            .or_else(|| ready.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_units() {
        let mut s = Snapshot::new(ProviderId::Cursor, Unit::Cents);
        assert_eq!(s.fmt(25_000_000), "$250,000.00");
        assert_eq!(s.fmt(43134), "$431.34");
        s.unit = Unit::Basis;
        assert_eq!(s.fmt(3500), "35%");
    }

    #[test]
    fn unknown_cycle_does_not_claim_pace_or_reset_date() {
        let mut s = Snapshot::new(ProviderId::OpenCode, Unit::Basis);
        s.limit = 10_000;
        s.used = 3800;
        assert_eq!(s.pace_text(), "Usage pace unavailable");
        assert_eq!(s.of_text(), "of the subscription limit");
        assert_eq!(s.resets_text(), "Reset time not reported");
        assert_eq!(s.projected_fraction(), s.used_fraction());
    }

    #[test]
    fn signed_out_providers_are_skipped_when_scrolling() {
        assert_eq!(scroll_target(3, 120, &[0, 3]), Some(0));
        assert_eq!(scroll_target(0, -120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(0, 120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(2, -120, &[0, 3]), Some(3));
        assert_eq!(scroll_target(3, 120, &[3]), Some(3));
        assert_eq!(scroll_target(3, 120, &[]), None);
        assert_eq!(scroll_target(3, 0, &[3]), None);
    }
}
