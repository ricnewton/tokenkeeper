//! Cursor provider: browser sign-in, token refresh and usage fetching.
//!
//! These are Cursor's own (undocumented) endpoints, as used by the Cursor CLI
//! and dashboard, so field handling is deliberately defensive.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::crypto;
use crate::http;
use crate::model::{ProviderId, Snapshot, Unit};
use crate::provider::{FetchError, Login, PollResult, Provider};
use crate::store::{self, save_debug};
use crate::time;

const LOGIN_URL: &str = "https://cursor.com/loginDeepControl";
const POLL_URL: &str = "https://api2.cursor.sh/auth/poll";
const REFRESH_URL: &str = "https://api2.cursor.sh/oauth/token";
const USAGE_SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const EVENTS_URL: &str = "https://cursor.com/api/dashboard/get-filtered-usage-events";
const CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LOGIN_POLL_EVERY: Duration = Duration::from_secs(2);
/// Refresh this long before the access token's `exp`.
const REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;
const STORE: &str = "cursor";

#[derive(Clone)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
}

impl Tokens {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Tokens { access: v["access"].as_str()?.into(), refresh: v["refresh"].as_str()?.into() })
    }

    fn save(&self) -> Result<(), String> {
        store::save_secret(STORE, &json!({ "access": self.access, "refresh": self.refresh }))
    }
}

#[derive(Default)]
pub struct Cursor {
    tokens: Option<Tokens>,
    breakdown: Breakdown,
}

impl Provider for Cursor {
    fn id(&self) -> ProviderId {
        ProviderId::Cursor
    }

    fn load(&mut self) -> bool {
        self.tokens = store::load_secret(STORE).as_ref().and_then(Tokens::from_json);
        self.tokens.is_some()
    }

    fn begin_login(&mut self) -> Result<Box<dyn Login>, String> {
        Ok(Box::new(begin_login()))
    }

    fn accept_login(&mut self, creds: Value) -> Result<(), String> {
        let t = Tokens::from_json(&creds).ok_or("Sign-in response had no tokens")?;
        self.breakdown = Breakdown::default();
        let saved = t.save();
        self.tokens = Some(t);
        saved
    }

    fn sign_out(&mut self) {
        store::delete_secret(STORE);
        self.tokens = None;
        self.breakdown = Breakdown::default();
    }

    fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError> {
        let t = self.tokens.as_mut().ok_or(FetchError::SignedOut)?;
        let (mut snap, changed) = fetch_summary(t, &self.breakdown)?;
        if changed {
            let _ = t.save();
        }
        send(snap.clone());
        sync_breakdown(t, &mut self.breakdown, &mut snap);
        send(snap);
        Ok(())
    }
}

fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&crypto::b64url_decode(payload)?).ok()
}

fn expires_ms(token: &str) -> Option<i64> {
    jwt_claims(token)?["exp"].as_i64().map(|s| s * 1000)
}

/// The `sub` claim looks like `auth0|user_01ABC…`; the cookie wants the last part.
fn user_id(token: &str) -> Option<String> {
    let sub = jwt_claims(token)?["sub"].as_str()?.to_string();
    Some(sub.rsplit('|').next().unwrap_or(&sub).to_string())
}

/// Starts the browser sign-in and returns the URL to open plus the secrets
/// needed to redeem it.
pub struct LoginAttempt {
    pub url: String,
    uuid: String,
    verifier: String,
    started: Instant,
}

pub fn begin_login() -> LoginAttempt {
    let verifier = crypto::b64url_encode(&crypto::random_bytes::<32>());
    let challenge = crypto::b64url_encode(&crypto::sha256(verifier.as_bytes()));
    let uuid = crypto::uuid_v4();
    let url = format!("{LOGIN_URL}?challenge={challenge}&uuid={uuid}&mode=login&redirectTarget=cli");
    LoginAttempt { url, uuid, verifier, started: Instant::now() }
}

impl Login for LoginAttempt {
    fn url(&self) -> &str {
        &self.url
    }

    fn poll_interval(&self) -> Duration {
        LOGIN_POLL_EVERY
    }

    /// One poll of the login endpoint; 404 means the browser hasn't finished.
    fn poll(&mut self) -> PollResult {
        if self.started.elapsed() > LOGIN_TIMEOUT {
            return PollResult::Failed("Sign-in timed out".into());
        }
        let url = format!("{POLL_URL}?uuid={}&verifier={}", self.uuid, self.verifier);
        match http::get(&url, &[]) {
            Ok(r) if r.status == 404 => PollResult::Pending,
            Ok(r) if r.status == 200 => match r.json() {
                Ok(v) => match (v["accessToken"].as_str(), v["refreshToken"].as_str()) {
                    (Some(a), Some(rf)) => PollResult::Done(json!({ "access": a, "refresh": rf })),
                    _ => PollResult::Failed("Sign-in response had no tokens".into()),
                },
                Err(e) => PollResult::Failed(e),
            },
            Ok(r) => PollResult::Failed(format!("Sign-in failed (HTTP {})", r.status)),
            // Transient network trouble: keep waiting until the timeout.
            Err(_) => PollResult::Pending,
        }
    }
}

/// Refreshes the access token if it's close to expiry. Returns true if the
/// tokens changed and should be saved.
pub fn ensure_fresh(t: &mut Tokens, force: bool) -> Result<bool, FetchError> {
    let exp = expires_ms(&t.access).unwrap_or(0);
    if !force && exp - REFRESH_MARGIN_MS > time::now_ms() {
        return Ok(false);
    }
    let body = json!({ "grant_type": "refresh_token", "client_id": CLIENT_ID, "refresh_token": t.refresh });
    let r = http::post_json(REFRESH_URL, &[], &body)?;
    if r.status == 401 || r.status == 403 {
        return Err(FetchError::SignedOut);
    }
    if r.status != 200 {
        return Err(FetchError::Other(format!("Token refresh failed (HTTP {})", r.status)));
    }
    let v = r.json()?;
    if v["shouldLogout"].as_bool() == Some(true) {
        return Err(FetchError::SignedOut);
    }
    let access = v["access_token"].as_str().unwrap_or_default();
    if access.is_empty() {
        return Err(FetchError::SignedOut);
    }
    t.access = access.to_string();
    if let Some(rf) = v["refresh_token"].as_str().filter(|s| !s.is_empty()) {
        t.refresh = rf.to_string();
    }
    Ok(true)
}

fn cookie(t: &Tokens) -> Result<String, FetchError> {
    let uid = user_id(&t.access).ok_or(FetchError::SignedOut)?;
    Ok(format!("WorkosCursorSessionToken={uid}%3A%3A{}", t.access))
}

fn dashboard_get(t: &Tokens, url: &str) -> Result<http::Response, FetchError> {
    let c = cookie(t)?;
    Ok(http::get(url, &[("Cookie", &c), ("Referer", "https://cursor.com/dashboard")])?)
}

fn dashboard_post(t: &Tokens, url: &str, body: &Value) -> Result<http::Response, FetchError> {
    let c = cookie(t)?;
    let headers = [("Cookie", c.as_str()), ("Origin", "https://cursor.com"), ("Referer", "https://cursor.com/dashboard")];
    Ok(http::post_json(url, &headers, body)?)
}

/// Per-request usage events for the current cycle plus cached totals for
/// past cycles; kept by the worker between polls.
#[derive(Default)]
pub struct Breakdown {
    cycle_start: i64,
    /// (timestamp ms, charged cents), any order.
    events: Vec<(i64, f64)>,
    /// Events up to this timestamp have been fetched.
    synced_to: i64,
    /// Past cycle start (ms) → total cents. Persisted, since past cycles don't change.
    history: std::collections::BTreeMap<i64, u32>,
    history_loaded: bool,
}

/// Re-fetch this much recent history each poll, since an event's cost can
/// be filled in after it first appears.
const RESYNC_MS: i64 = 2 * 60 * 60 * 1000;
const HISTORY_CYCLES: i64 = 5;
const PAGE_SIZE: u64 = 100;
const MAX_PAGES: u64 = 300;

const HISTORY_FILE: &str = "cursor-history.json";

/// Fetches the current usage, refreshing the token once on a 401.
/// Returns the snapshot and whether `t` changed.
fn fetch_summary(t: &mut Tokens, b: &Breakdown) -> Result<(Snapshot, bool), FetchError> {
    let mut changed = ensure_fresh(t, false)?;
    let mut r = dashboard_get(t, USAGE_SUMMARY_URL)?;
    if r.status == 401 || r.status == 403 {
        changed |= ensure_fresh(t, true)?;
        r = dashboard_get(t, USAGE_SUMMARY_URL)?;
        if r.status == 401 || r.status == 403 {
            return Err(FetchError::SignedOut);
        }
    }
    if r.status != 200 {
        return Err(FetchError::Other(format!("Usage request failed (HTTP {})", r.status)));
    }
    let v = r.json()?;
    let mut snap = parse_summary(&v).ok_or_else(|| FetchError::Other("Unrecognised usage response".into()))?;
    apply_breakdown(&mut snap, b);
    Ok((snap, changed))
}

/// Brings the per-event breakdown up to date and applies it to `snap`.
/// Best-effort: on failure the previously cached breakdown is kept.
fn sync_breakdown(t: &Tokens, b: &mut Breakdown, snap: &mut Snapshot) {
    if let Err(e) = sync_events(t, b, snap.cycle_start_ms) {
        save_debug("last-breakdown-error.txt", e.as_bytes());
    }
    if let Err(e) = sync_history(t, b, snap.cycle_start_ms) {
        save_debug("last-history-error.txt", e.as_bytes());
    }
    apply_breakdown(snap, b);
}

/// All events in [start, end), newest first, across pages.
fn fetch_events(t: &Tokens, start: i64, end: i64) -> Result<Vec<(i64, f64)>, String> {
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let body = json!({
            "startDate": start.to_string(),
            "endDate": end.to_string(),
            "page": page,
            "pageSize": PAGE_SIZE,
        });
        let r = dashboard_post(t, EVENTS_URL, &body).map_err(|e| match e {
            FetchError::SignedOut => "signed out".to_string(),
            FetchError::Other(e) => e,
        })?;
        if r.status != 200 {
            return Err(format!("Usage events request failed (HTTP {})", r.status));
        }
        let v = r.json()?;
        let events = v["usageEventsDisplay"].as_array().cloned().unwrap_or_default();
        let total = v["totalUsageEventsCount"].as_u64().unwrap_or(0);
        for e in &events {
            let Some(ts) = e["timestamp"].as_str().and_then(|s| s.parse::<i64>().ok()) else { continue };
            let cents = num(&e["chargedCents"]).or_else(|| num(&e["tokenUsage"]["totalCents"])).unwrap_or(0.0);
            out.push((ts, cents));
        }
        if events.is_empty() || page * PAGE_SIZE >= total {
            break;
        }
    }
    Ok(out)
}

fn sync_events(t: &Tokens, b: &mut Breakdown, cycle_start: i64) -> Result<(), String> {
    let now = time::now_ms();
    if b.cycle_start != cycle_start {
        b.cycle_start = cycle_start;
        b.events.clear();
        b.synced_to = cycle_start;
    }
    let from = (b.synced_to - RESYNC_MS).max(cycle_start);
    let fresh = fetch_events(t, from, now)?;
    b.events.retain(|(ts, _)| *ts < from);
    b.events.extend(fresh);
    b.synced_to = now;
    Ok(())
}

fn sync_history(t: &Tokens, b: &mut Breakdown, cycle_start: i64) -> Result<(), String> {
    if !b.history_loaded {
        b.history_loaded = true;
        if let Some(Value::Object(map)) = store::load_json(HISTORY_FILE) {
            for (k, v) in map {
                if let (Ok(k), Some(v)) = (k.parse::<i64>(), v.as_u64()) {
                    b.history.insert(k, v as u32);
                }
            }
        }
    }
    let mut dirty = false;
    for i in 1..=HISTORY_CYCLES {
        let start = time::months_before(cycle_start, i);
        if b.history.contains_key(&start) {
            continue;
        }
        let end = time::months_before(cycle_start, i - 1);
        let total: f64 = fetch_events(t, start, end)?.iter().map(|(_, c)| c).sum();
        b.history.insert(start, total.round() as u32);
        dirty = true;
    }
    if dirty {
        let map: serde_json::Map<String, Value> = b.history.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        store::save_json(HISTORY_FILE, &Value::Object(map));
    }
    Ok(())
}

fn apply_breakdown(s: &mut Snapshot, b: &Breakdown) {
    if b.cycle_start != s.cycle_start_ms || b.synced_to == 0 {
        return;
    }
    let now = time::now_ms();
    let day = |ms: i64| time::local_wall_ms(ms).div_euclid(time::DAY_MS);
    let first = day(s.cycle_start_ms);
    let last = day(s.cycle_end_ms - 1);
    let today = day(now);
    let days = (last - first + 1).max(1) as usize;
    let mut daily = vec![0f64; days];
    let mut hourly = vec![0f64; 24];
    for &(ts, cents) in &b.events {
        let d = day(ts);
        if (first..=last).contains(&d) {
            daily[(d - first) as usize] += cents;
        }
        if d == today {
            let hour = time::local_wall_ms(ts).rem_euclid(time::DAY_MS) / 3_600_000;
            hourly[hour as usize] += cents;
        }
    }
    s.daily = daily
        .iter()
        .enumerate()
        .map(|(i, c)| (first + i as i64 <= today).then(|| c.round() as u32))
        .collect();
    s.daily_labels = (0..days as i64)
        .map(|i| {
            let t = time::local(s.cycle_start_ms + i * time::DAY_MS);
            format!("{}/{}", t.wDay, t.wMonth)
        })
        .collect();
    let now_hour = time::local_wall_ms(now).rem_euclid(time::DAY_MS) / 3_600_000;
    s.hourly = hourly.iter().enumerate().map(|(h, c)| (h as i64 <= now_hour).then(|| c.round() as u32)).collect();
    s.today = Some(hourly.iter().sum::<f64>().round() as u32);

    let mut history: Vec<(i64, u32)> = (1..=HISTORY_CYCLES)
        .rev()
        .filter_map(|i| {
            let start = time::months_before(s.cycle_start_ms, i);
            b.history.get(&start).map(|v| (start, *v))
        })
        .collect();
    history.push((s.cycle_start_ms, s.used));
    s.history_labels = history.iter().map(|(start, _)| time::month(*start + time::DAY_MS).to_string()).collect();
    s.history = history.into_iter().map(|(_, v)| v).collect();
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

fn parse_summary(v: &Value) -> Option<Snapshot> {
    let start = time::parse_timestamp_ms(v["billingCycleStart"].as_str()?)?;
    let end = time::parse_timestamp_ms(v["billingCycleEnd"].as_str()?)?;
    let plan = v["membershipType"].as_str().unwrap_or("cursor");
    let plan = {
        let mut c = plan.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
    };

    // Pro-style plans report `individualUsage.plan`; enterprise seats report
    // `individualUsage.overall`; some teams only have a pooled limit.
    let candidates = [
        &v["individualUsage"]["plan"],
        &v["individualUsage"]["overall"],
        &v["teamUsage"]["pooled"],
        &v["teamUsage"]["plan"],
    ];
    let (used, limit) = candidates
        .iter()
        .find_map(|p| {
            let limit = num(&p["limit"])?;
            (limit > 0.0).then(|| (num(&p["used"]).unwrap_or(0.0), limit))
        })
        .unwrap_or((0.0, 0.0));

    let mut s = Snapshot::new(ProviderId::Cursor, Unit::Cents);
    s.plan = plan;
    s.limit = limit as u32;
    s.used = used as u32;
    s.cycle_start_ms = start;
    s.cycle_end_ms = end;
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_enterprise_summary() {
        let v: Value = serde_json::from_str(
            r#"{
              "billingCycleStart": "2026-04-02T14:11:55.000Z",
              "billingCycleEnd": "2026-05-02T14:11:55.000Z",
              "membershipType": "enterprise",
              "individualUsage": { "plan": { "enabled": true, "used": 840, "limit": 2000, "remaining": 1160 } }
            }"#,
        )
        .unwrap();
        let s = parse_summary(&v).unwrap();
        assert_eq!((s.used, s.limit, s.plan.as_str()), (840, 2000, "Enterprise"));
        assert_eq!(s.cycle_end_ms - s.cycle_start_ms, 30 * time::DAY_MS);
    }

    #[test]
    fn parses_enterprise_overall_summary() {
        let v: Value = serde_json::from_str(
            r#"{
              "billingCycleStart": "2026-10-01T00:00:00.000Z",
              "billingCycleEnd": "2026-11-01T00:00:00.000Z",
              "membershipType": "enterprise",
              "limitType": "team",
              "individualUsage": { "overall": { "enabled": true, "used": 43134, "limit": 250000, "remaining": 206866 } },
              "teamUsage": { "onDemand": { "enabled": true, "used": 0, "limit": 1500000, "remaining": 1500000 } }
            }"#,
        )
        .unwrap();
        let s = parse_summary(&v).unwrap();
        assert_eq!((s.used, s.limit), (43134, 250000));
        assert_eq!(s.fmt(s.used), "$431.34");
    }

    #[test]
    fn user_id_from_sub() {
        let payload = crypto::b64url_encode(br#"{"sub":"auth0|user_01ABC","exp":2000000000}"#);
        let token = format!("x.{payload}.y");
        assert_eq!(user_id(&token).as_deref(), Some("user_01ABC"));
        assert_eq!(expires_ms(&token), Some(2_000_000_000_000));
    }
}
