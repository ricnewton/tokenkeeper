//! GitHub Copilot provider.
//!
//! Signs in with GitHub's device flow using the Copilot editor client ID (the
//! only client the Copilot endpoints accept), then reads the same quota
//! snapshot the editors use for their "credits used" badge. A credit is worth
//! a cent, so usage is shown in dollars. Copilot reports only the month's
//! running total, so the breakdown comes from `Tracker`.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::http;
use crate::model::{Detail, ProviderId, Snapshot, Unit};
use crate::provider::{FetchError, Login, PollResult, Provider};
use crate::store::{self, save_debug};
use crate::time;
use crate::tracker::Tracker;

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/copilot_internal/user";
const STORE: &str = "copilot";

/// GitHub App user tokens may expire (with a refresh token) or be long-lived.
#[derive(Clone)]
struct Tokens {
    access: String,
    refresh: Option<String>,
    /// 0 if the token doesn't expire.
    expires_ms: i64,
}

impl Tokens {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Tokens {
            access: v["token"].as_str()?.into(),
            refresh: v["refresh"].as_str().map(String::from),
            expires_ms: v["expires_ms"].as_i64().unwrap_or(0),
        })
    }

    fn to_json(&self) -> Value {
        json!({ "token": self.access, "refresh": self.refresh, "expires_ms": self.expires_ms })
    }

    /// From a GitHub token endpoint response.
    fn from_grant(v: &Value) -> Option<Self> {
        let expires_in = v["expires_in"].as_i64().unwrap_or(0);
        Some(Tokens {
            access: v["access_token"].as_str()?.into(),
            refresh: v["refresh_token"].as_str().map(String::from),
            expires_ms: if expires_in > 0 { time::now_ms() + expires_in * 1000 } else { 0 },
        })
    }
}

const REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;

pub struct Copilot {
    tokens: Option<Tokens>,
    tracker: Tracker,
}

impl Default for Copilot {
    fn default() -> Self {
        Copilot { tokens: None, tracker: Tracker::new("copilot-history.json") }
    }
}

impl Provider for Copilot {
    fn id(&self) -> ProviderId {
        ProviderId::Copilot
    }

    fn load(&mut self) -> bool {
        self.tokens = store::load_secret(STORE).as_ref().and_then(Tokens::from_json);
        self.tokens.is_some()
    }

    fn begin_login(&mut self) -> Result<Box<dyn Login>, String> {
        let r = http::post_json(DEVICE_CODE_URL, &[("Accept", "application/json")], &json!({ "client_id": CLIENT_ID, "scope": "read:user" }))?;
        if r.status != 200 {
            save_debug("copilot-device-error.txt", &r.body);
        }
        let v = r.json()?;
        let field = |k: &str| v[k].as_str().map(String::from).ok_or(format!("GitHub sign-in failed (HTTP {})", r.status));
        Ok(Box::new(DeviceLogin {
            device_code: field("device_code")?,
            user_code: field("user_code")?,
            url: field("verification_uri")?,
            interval: Duration::from_secs(v["interval"].as_u64().unwrap_or(5).max(1)),
            expires: Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(900)),
        }))
    }

    fn accept_login(&mut self, creds: Value) -> Result<(), String> {
        let t = Tokens::from_json(&creds).ok_or("Sign-in response had no token")?;
        self.tracker.reset();
        let saved = store::save_secret(STORE, &t.to_json());
        self.tokens = Some(t);
        saved
    }

    fn sign_out(&mut self) {
        store::delete_secret(STORE);
        self.tokens = None;
        self.tracker.reset();
    }

    fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError> {
        let t = self.tokens.as_mut().ok_or(FetchError::SignedOut)?;
        let mut changed = ensure_fresh(t, false)?;
        let mut r = get_user(t)?;
        if r.status == 401 && t.refresh.is_some() {
            changed |= ensure_fresh(t, true)?;
            r = get_user(t)?;
        }
        if changed {
            let _ = store::save_secret(STORE, &t.to_json());
        }
        if r.status == 401 {
            save_debug("copilot-user-error.txt", &r.body);
            return Err(FetchError::SignedOut);
        }
        if r.status != 200 {
            save_debug("copilot-user-error.txt", &r.body);
            return Err(FetchError::Other(format!("Copilot request failed (HTTP {})", r.status)));
        }
        let mut s = parse_user(&r.json()?).ok_or_else(|| {
            save_debug("copilot-user.json", &r.body);
            FetchError::Other("Unrecognised Copilot response".into())
        })?;
        self.tracker.record(&mut s, |start| time::month(start + time::DAY_MS).to_string());
        send(s);
        Ok(())
    }
}

fn get_user(t: &Tokens) -> Result<http::Response, FetchError> {
    let auth = format!("token {}", t.access);
    Ok(http::get(USER_URL, &[("Authorization", &auth), ("Accept", "application/json"), ("X-GitHub-Api-Version", "2025-04-01")])?)
}

/// Refreshes an expiring token near expiry (or when forced). Returns true if it changed.
fn ensure_fresh(t: &mut Tokens, force: bool) -> Result<bool, FetchError> {
    let Some(refresh) = t.refresh.clone() else { return Ok(false) };
    if !force && (t.expires_ms == 0 || t.expires_ms - REFRESH_MARGIN_MS > time::now_ms()) {
        return Ok(false);
    }
    let body = json!({ "client_id": CLIENT_ID, "grant_type": "refresh_token", "refresh_token": refresh });
    let r = http::post_json(TOKEN_URL, &[("Accept", "application/json")], &body)?;
    let v = r.json()?;
    match Tokens::from_grant(&v) {
        Some(fresh) => {
            t.access = fresh.access;
            t.expires_ms = fresh.expires_ms;
            t.refresh = fresh.refresh.or(Some(refresh));
            Ok(true)
        }
        None if v["error"].as_str() == Some("bad_refresh_token") => Err(FetchError::SignedOut),
        None => {
            save_debug("copilot-refresh-error.txt", &r.body);
            Err(FetchError::Other(format!("GitHub token refresh failed (HTTP {})", r.status)))
        }
    }
}

struct DeviceLogin {
    device_code: String,
    user_code: String,
    url: String,
    interval: Duration,
    expires: Instant,
}

impl Login for DeviceLogin {
    fn url(&self) -> &str {
        &self.url
    }

    fn user_code(&self) -> Option<&str> {
        Some(&self.user_code)
    }

    fn poll_interval(&self) -> Duration {
        self.interval
    }

    fn poll(&mut self) -> PollResult {
        if Instant::now() > self.expires {
            return PollResult::Failed("Sign-in code expired".into());
        }
        let body = json!({
            "client_id": CLIENT_ID,
            "device_code": self.device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        });
        let r = match http::post_json(TOKEN_URL, &[("Accept", "application/json")], &body) {
            Ok(r) => r,
            Err(_) => return PollResult::Pending,
        };
        let Ok(v) = r.json() else {
            save_debug("copilot-login-error.txt", &r.body);
            return PollResult::Pending;
        };
        if let Some(t) = Tokens::from_grant(&v) {
            return PollResult::Done(t.to_json());
        }
        if v["error"].as_str() != Some("authorization_pending") {
            save_debug("copilot-login-error.txt", &r.body);
        }
        match v["error"].as_str() {
            Some("authorization_pending") | None => PollResult::Pending,
            Some("slow_down") => {
                self.interval += Duration::from_secs(5);
                PollResult::Pending
            }
            Some("access_denied") => PollResult::Failed("Sign-in was cancelled on GitHub".into()),
            Some("expired_token") => PollResult::Failed("Sign-in code expired".into()),
            Some(e) => PollResult::Failed(format!("GitHub sign-in failed: {e}")),
        }
    }
}

fn parse_user(v: &Value) -> Option<Snapshot> {
    let q = &v["quota_snapshots"]["premium_interactions"];
    if !q.is_object() {
        return None;
    }
    let num = |k: &str| q[k].as_f64();
    let limit = num("entitlement").unwrap_or(0.0).max(0.0);
    let used = num("credits_used")
        .or_else(|| Some(limit - num("quota_remaining").or(num("remaining"))?))
        .unwrap_or(0.0)
        .max(0.0);
    let end = v["quota_reset_date_utc"]
        .as_str()
        .or(v["quota_reset_date"].as_str())
        .and_then(time::parse_timestamp_ms)?;

    let mut s = Snapshot::new(ProviderId::Copilot, Unit::Cents);
    let plan = v["copilot_plan"].as_str().unwrap_or("copilot");
    s.plan = plan.split('_').map(capitalise).collect::<Vec<_>>().join(" ");
    s.limit = limit.round() as u32;
    s.used = used.round() as u32;
    s.cycle_end_ms = end;
    s.cycle_start_ms = time::months_before(end, 1);
    s.details = vec![Detail::Today, Detail::Cycle, Detail::History];
    Some(s)
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enterprise_seat() {
        let v: Value = serde_json::from_str(
            r#"{
              "copilot_plan": "enterprise",
              "quota_reset_date": "2026-11-01",
              "quota_reset_date_utc": "2026-11-01T00:00:00.000Z",
              "quota_snapshots": {
                "chat": { "entitlement": 0, "unlimited": true, "percent_remaining": 100.0 },
                "premium_interactions": {
                  "entitlement": 10000, "percent_remaining": 46.5, "quota_remaining": 4653.7,
                  "remaining": 4653, "unlimited": false, "credits_used": 5346, "token_based_billing": true
                }
              }
            }"#,
        )
        .unwrap();
        let s = parse_user(&v).unwrap();
        assert_eq!((s.used, s.limit, s.plan.as_str()), (5346, 10000, "Enterprise"));
        assert_eq!((s.fmt(s.used), s.of_text()), ("$53.46".into(), "of $100.00 included".into()));
        assert_eq!(s.cycle_end_ms - s.cycle_start_ms, 31 * time::DAY_MS);
    }

    #[test]
    fn derives_used_without_credits_field() {
        let v: Value = serde_json::from_str(
            r#"{
              "copilot_plan": "individual_pro",
              "quota_reset_date": "2026-05-01",
              "quota_snapshots": { "premium_interactions": { "entitlement": 300, "quota_remaining": 93.5 } }
            }"#,
        )
        .unwrap();
        let s = parse_user(&v).unwrap();
        assert_eq!((s.used, s.limit, s.plan.as_str()), (207, 300, "Individual Pro"));
    }
}
