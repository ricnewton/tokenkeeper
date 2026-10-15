//! Claude Code provider.
//!
//! Signs in like Claude Code does (OAuth PKCE on claude.ai with a localhost
//! callback), asking only for the `user:profile` scope that the usage
//! endpoint needs, so these tokens can't run inference. Usage comes from the
//! endpoint behind Claude Code's `/usage` command: percentages of rolling
//! rate-limit windows rather than an amount spent.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::crypto;
use crate::http;
use crate::model::{Detail, Meter, ProviderId, Snapshot, Unit};
use crate::provider::{FetchError, Login, PollResult, Provider};
use crate::store::{self, save_debug};
use crate::time::{self, DAY_MS};
use crate::tracker::Tracker;

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const SCOPE: &str = "user:profile";
const BETA: &str = "oauth-2025-04-20";
const STORE: &str = "claude";
/// Anthropic's endpoints answer 429 to browser-like agents that aren't browsers.
const AGENT: (&str, &str) = ("User-Agent", concat!("tokenkeeper/", env!("CARGO_PKG_VERSION")));
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;

#[derive(Clone)]
struct Tokens {
    access: String,
    refresh: String,
    expires_ms: i64,
    plan: Option<String>,
}

impl Tokens {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Tokens {
            access: v["access"].as_str()?.into(),
            refresh: v["refresh"].as_str()?.into(),
            expires_ms: v["expires_ms"].as_i64().unwrap_or(0),
            plan: v["plan"].as_str().map(String::from),
        })
    }

    fn to_json(&self) -> Value {
        json!({ "access": self.access, "refresh": self.refresh, "expires_ms": self.expires_ms, "plan": self.plan })
    }

    /// From a token endpoint response.
    fn from_grant(v: &Value, old_refresh: Option<&str>) -> Option<Self> {
        Some(Tokens {
            access: v["access_token"].as_str()?.into(),
            refresh: v["refresh_token"].as_str().or(old_refresh)?.into(),
            expires_ms: time::now_ms() + v["expires_in"].as_i64().unwrap_or(3600) * 1000,
            plan: None,
        })
    }
}

pub struct Claude {
    tokens: Option<Tokens>,
    tracker: Tracker,
}

impl Default for Claude {
    fn default() -> Self {
        Claude { tokens: None, tracker: Tracker::new("claude-history.json") }
    }
}

impl Provider for Claude {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn load(&mut self) -> bool {
        self.tokens = store::load_secret(STORE).as_ref().and_then(Tokens::from_json);
        self.tokens.is_some()
    }

    fn begin_login(&mut self) -> Result<Box<dyn Login>, String> {
        Ok(Box::new(BrowserLogin::start()?))
    }

    fn accept_login(&mut self, creds: Value) -> Result<(), String> {
        let t = Tokens::from_json(&creds).ok_or("Sign-in response had no tokens")?;
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
        let mut r = api_get(t, USAGE_URL)?;
        if r.status == 401 {
            changed |= ensure_fresh(t, true)?;
            r = api_get(t, USAGE_URL)?;
            if r.status == 401 {
                return Err(FetchError::SignedOut);
            }
        }
        if t.plan.is_none() {
            t.plan = fetch_plan(t);
            changed |= t.plan.is_some();
        }
        if changed {
            let _ = store::save_secret(STORE, &t.to_json());
        }
        if r.status != 200 {
            save_debug("claude-usage-error.txt", &r.body);
            let what = if r.status == 429 { "rate limited".to_string() } else { format!("HTTP {}", r.status) };
            return Err(FetchError::Other(format!("Claude usage request failed ({what})")));
        }
        let mut s = parse_usage(&r.json()?).ok_or_else(|| {
            save_debug("claude-usage.json", &r.body);
            FetchError::Other("Unrecognised Claude usage response".into())
        })?;
        s.plan = t.plan.clone().unwrap_or_else(|| "Claude".into());
        let weekly = s.unit == Unit::Basis;
        self.tracker.record(&mut s, |start| if weekly { time::day_month(start) } else { time::month(start + DAY_MS).into() });
        send(s);
        Ok(())
    }
}

fn api_get(t: &Tokens, url: &str) -> Result<http::Response, FetchError> {
    let auth = format!("Bearer {}", t.access);
    Ok(http::get(url, &[AGENT, ("Authorization", &auth), ("anthropic-beta", BETA), ("Accept", "application/json")])?)
}

fn ensure_fresh(t: &mut Tokens, force: bool) -> Result<bool, FetchError> {
    if !force && t.expires_ms - REFRESH_MARGIN_MS > time::now_ms() {
        return Ok(false);
    }
    let body = json!({ "grant_type": "refresh_token", "refresh_token": t.refresh, "client_id": CLIENT_ID });
    let r = http::post_json(TOKEN_URL, &[AGENT], &body)?;
    if matches!(r.status, 400 | 401 | 403) {
        return Err(FetchError::SignedOut);
    }
    if r.status != 200 {
        return Err(FetchError::Other(format!("Claude token refresh failed (HTTP {})", r.status)));
    }
    let fresh = Tokens::from_grant(&r.json()?, Some(&t.refresh)).ok_or(FetchError::SignedOut)?;
    t.access = fresh.access;
    t.refresh = fresh.refresh;
    t.expires_ms = fresh.expires_ms;
    Ok(true)
}

/// Best-effort plan name, e.g. "Max" or "Enterprise".
fn fetch_plan(t: &Tokens) -> Option<String> {
    let r = api_get(t, PROFILE_URL).ok().filter(|r| r.status == 200)?;
    let v = r.json().ok()?;
    let org = v["organization"]["organization_type"].as_str().unwrap_or_default();
    let plan = match org {
        o if o.contains("enterprise") => "Enterprise",
        o if o.contains("team") => "Team",
        o if o.contains("max") => "Max",
        o if o.contains("pro") => "Pro",
        _ if v["account"]["has_claude_max"].as_bool() == Some(true) => "Max",
        _ if v["account"]["has_claude_pro"].as_bool() == Some(true) => "Pro",
        _ => return None,
    };
    Some(plan.into())
}

/// A rate-limit window from the usage response, if present.
fn window(v: &Value) -> Option<(f32, Option<i64>)> {
    let util = v["utilization"].as_f64()?;
    Some((util as f32 / 100.0, v["resets_at"].as_str().and_then(time::parse_timestamp_ms)))
}

fn parse_usage(v: &Value) -> Option<Snapshot> {
    let named = [
        ("five_hour", "5-hour session", 5 * 3_600_000),
        ("seven_day", "Weekly, all models", 7 * DAY_MS),
        ("seven_day_opus", "Weekly, Opus", 7 * DAY_MS),
        ("seven_day_sonnet", "Weekly, Sonnet", 7 * DAY_MS),
    ];
    let mut meters = Vec::new();
    let mut primary: Option<(f32, i64, i64)> = None;
    for (key, label, span) in named {
        let Some((fraction, resets)) = window(&v[key]) else { continue };
        meters.push(Meter { label: label.into(), fraction, resets_ms: resets });
        if key == "seven_day" || (primary.is_none() && key == "five_hour") {
            let end = resets.unwrap_or_else(|| time::now_ms() + span);
            primary = Some((fraction, end - span, end));
        }
    }
    for lim in v["limits"].as_array().into_iter().flatten() {
        if lim["kind"].as_str() != Some("weekly_scoped") {
            continue;
        }
        let Some(pct) = lim["percent"].as_f64() else { continue };
        let model = lim["scope"]["model"]["display_name"].as_str().unwrap_or("scoped");
        if meters.iter().any(|m| m.label.ends_with(model)) {
            continue;
        }
        let resets_ms = lim["resets_at"].as_str().and_then(time::parse_timestamp_ms);
        meters.push(Meter { label: format!("Weekly, {model}"), fraction: pct as f32 / 100.0, resets_ms });
    }

    let extra = &v["extra_usage"];
    let extra_used = extra["used_credits"].as_f64();
    let extra_limit = extra["monthly_limit"].as_f64().filter(|l| *l > 0.0);
    // Without rate-limit windows the spend is already the headline ring.
    if primary.is_some() && extra["is_enabled"].as_bool() == Some(true) {
        if let (Some(used), Some(limit)) = (extra_used, extra_limit) {
            meters.push(Meter { label: "Extra usage this month".into(), fraction: (used / limit) as f32, resets_ms: None });
        }
    }

    let mut s;
    if let Some((fraction, start, end)) = primary {
        s = Snapshot::new(ProviderId::Claude, Unit::Basis);
        s.limit = 10_000;
        s.used = (fraction * 10_000.0).round().max(0.0) as u32;
        s.cycle_start_ms = start;
        s.cycle_end_ms = end;
    } else if let (Some(used), Some(limit)) = (extra_used, extra_limit) {
        // Spend-capped seats without rate-limit windows.
        s = Snapshot::new(ProviderId::Claude, Unit::Cents);
        s.limit = limit.round() as u32;
        s.used = used.round().max(0.0) as u32;
        let now = time::local(time::now_ms());
        let month_start = time::parse_timestamp_ms(&format!("{:04}-{:02}-01", now.wYear, now.wMonth))?;
        s.cycle_start_ms = month_start;
        s.cycle_end_ms = time::months_before(month_start, -1);
    } else {
        return None;
    }
    // The headline window is already the ring; list the others first.
    meters.sort_by_key(|m| m.label != "5-hour session");
    s.meters = meters;
    s.details = vec![Detail::Limits, Detail::Cycle, Detail::History];
    Some(s)
}

/// Browser sign-in: claude.ai redirects back to a one-shot listener on localhost.
struct BrowserLogin {
    url: String,
    listener: TcpListener,
    redirect_uri: String,
    verifier: String,
    state: String,
    started: Instant,
}

impl BrowserLogin {
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Couldn't start sign-in listener: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let redirect_uri = format!("http://localhost:{port}/callback");
        let verifier = crypto::b64url_encode(&crypto::random_bytes::<32>());
        let challenge = crypto::b64url_encode(&crypto::sha256(verifier.as_bytes()));
        let state = crypto::b64url_encode(&crypto::random_bytes::<32>());
        let url = format!(
            "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
            pct_encode(&redirect_uri),
            pct_encode(SCOPE),
        );
        Ok(BrowserLogin { url, listener, redirect_uri, verifier, state, started: Instant::now() })
    }

    /// Handles one browser request; returns None to keep waiting.
    fn handle(&self, mut stream: TcpStream) -> Option<PollResult> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let head = String::from_utf8_lossy(&buf);
        let target = head.lines().next()?.split(' ').nth(1)?.to_string();
        let Some(query) = target.strip_prefix("/callback?") else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return None;
        };
        let param = |k: &str| {
            query.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('=')).map(pct_decode)
        };
        let result = if let Some(e) = param("error") {
            PollResult::Failed(format!("Claude sign-in failed: {e}"))
        } else if param("state").as_deref() != Some(self.state.as_str()) {
            PollResult::Failed("Claude sign-in failed: state mismatch".into())
        } else if let Some(code) = param("code") {
            self.exchange(&code)
        } else {
            PollResult::Failed("Claude sign-in returned no code".into())
        };
        let (title, body) = match &result {
            PollResult::Done(_) => ("Signed in to Claude", "You can close this tab. Usage will appear in the tray shortly."),
            _ => ("Sign-in failed", "Something went wrong. Try signing in again from the tray icon."),
        };
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px Segoe UI,sans-serif;margin:15vh auto;max-width:28em;text-align:center\"><h2>{title}</h2><p>{body}</p>"
        );
        let _ = stream.write_all(
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len())
                .as_bytes(),
        );
        Some(result)
    }

    fn exchange(&self, code: &str) -> PollResult {
        // The pasted-code flow appends "#state"; the redirect flow shouldn't, but be lenient.
        let code = code.split('#').next().unwrap_or(code);
        let body = json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": self.redirect_uri,
            "client_id": CLIENT_ID,
            "code_verifier": self.verifier,
            "state": self.state,
        });
        match http::post_json(TOKEN_URL, &[AGENT], &body) {
            Ok(r) if r.status == 200 => match r.json().ok().as_ref().and_then(|v| Tokens::from_grant(v, None)) {
                Some(t) => PollResult::Done(t.to_json()),
                None => PollResult::Failed("Claude sign-in response had no tokens".into()),
            },
            Ok(r) => {
                save_debug("claude-login-error.txt", &r.body);
                PollResult::Failed(format!("Claude sign-in failed (HTTP {})", r.status))
            }
            Err(e) => PollResult::Failed(e),
        }
    }
}

impl Login for BrowserLogin {
    fn url(&self) -> &str {
        &self.url
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(250)
    }

    fn poll(&mut self) -> PollResult {
        if self.started.elapsed() > LOGIN_TIMEOUT {
            return PollResult::Failed("Sign-in timed out".into());
        }
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(r) = self.handle(stream) {
                        return r;
                    }
                }
                Err(_) => return PollResult::Pending,
            }
        }
    }
}

fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limit_windows() {
        let v: Value = serde_json::from_str(
            r#"{
              "five_hour": { "utilization": 33.0, "resets_at": "2026-04-11T07:00:00.528743+00:00" },
              "seven_day": { "utilization": 13.0, "resets_at": "2026-04-17T00:59:59.951713+00:00" },
              "seven_day_opus": null,
              "seven_day_sonnet": { "utilization": 1.0, "resets_at": "2026-04-16T03:00:00.951719+00:00" },
              "extra_usage": { "is_enabled": false, "monthly_limit": null, "used_credits": null }
            }"#,
        )
        .unwrap();
        let s = parse_usage(&v).unwrap();
        assert_eq!((s.unit, s.used, s.limit), (Unit::Basis, 1300, 10_000));
        assert_eq!(s.cycle_end_ms - s.cycle_start_ms, 7 * DAY_MS);
        let labels: Vec<&str> = s.meters.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["5-hour session", "Weekly, all models", "Weekly, Sonnet"]);
    }

    #[test]
    fn falls_back_to_extra_usage_spend() {
        let v: Value = serde_json::from_str(
            r#"{ "five_hour": null, "seven_day": null,
                 "extra_usage": { "is_enabled": true, "used_credits": 500, "monthly_limit": 10000 } }"#,
        )
        .unwrap();
        let s = parse_usage(&v).unwrap();
        assert_eq!((s.unit, s.used, s.limit), (Unit::Cents, 500, 10_000));
        assert!(s.meters.is_empty(), "spend is the ring, not also a meter");
    }

    #[test]
    fn lists_extra_usage_beside_rate_limit_windows() {
        let v: Value = serde_json::from_str(
            r#"{ "seven_day": { "utilization": 40.0, "resets_at": "2026-04-17T00:59:59Z" },
                 "extra_usage": { "is_enabled": true, "used_credits": 2500, "monthly_limit": 10000 } }"#,
        )
        .unwrap();
        let s = parse_usage(&v).unwrap();
        let labels: Vec<&str> = s.meters.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["Weekly, all models", "Extra usage this month"]);
    }

    #[test]
    fn query_decoding() {
        assert_eq!(pct_decode("a%2Bb+c%3D"), "a+b c=");
        assert_eq!(pct_encode("http://localhost:1/cb"), "http%3A%2F%2Flocalhost%3A1%2Fcb");
    }
}
