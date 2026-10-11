//! ChatGPT Codex subscription usage via browser OAuth and supplier HTTPS APIs.
//! Credentials belong to this app and never depend on a local Codex installation.
use crate::model::{Detail, Meter, ProviderId, Snapshot, Unit};
use crate::provider::{FetchError, Login, PollResult, Provider};
use crate::{crypto, http, store, time};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const STORE: &str = "codex";
const AGENT: (&str, &str) = (
    "User-Agent",
    concat!("tokenkeeper/", env!("CARGO_PKG_VERSION")),
);

#[derive(Clone)]
struct Tokens {
    access: String,
    refresh: String,
    account: String,
    expires_ms: i64,
}
impl Tokens {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            access: v["access"].as_str()?.into(),
            refresh: v["refresh"].as_str()?.into(),
            account: v["account"].as_str()?.into(),
            expires_ms: v["expires_ms"].as_i64()?,
        })
    }
    fn to_json(&self) -> Value {
        json!({"access":self.access,"refresh":self.refresh,"account":self.account,"expires_ms":self.expires_ms})
    }
    fn from_grant(v: &Value, old: Option<&Self>) -> Option<Self> {
        let access = v["access_token"].as_str()?.to_owned();
        let claims = v["id_token"]
            .as_str()
            .and_then(jwt_claims)
            .or_else(|| jwt_claims(&access));
        let account = claims
            .as_ref()
            .and_then(|c| c["https://api.openai.com/auth"]["chatgpt_account_id"].as_str())
            .or_else(|| old.map(|t| t.account.as_str()))?
            .to_owned();
        let expires_ms = jwt_claims(&access)
            .and_then(|c| c["exp"].as_i64())
            .and_then(|e| e.checked_mul(1000))
            .or_else(|| {
                v["expires_in"]
                    .as_i64()
                    .filter(|s| *s > 0)
                    .and_then(|s| s.checked_mul(1000))
                    .and_then(|s| time::now_ms().checked_add(s))
            })
            .unwrap_or_else(|| time::now_ms() + 3_600_000);
        Some(Self {
            access,
            refresh: v["refresh_token"]
                .as_str()
                .or_else(|| old.map(|t| t.refresh.as_str()))?
                .into(),
            account,
            expires_ms,
        })
    }
}
fn jwt_claims(token: &str) -> Option<Value> {
    serde_json::from_slice(&crypto::b64url_decode(token.split('.').nth(1)?)?).ok()
}
#[derive(Default)]
pub struct Codex {
    tokens: Option<Tokens>,
    dirty: bool,
}
impl Provider for Codex {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }
    fn load(&mut self) -> bool {
        self.tokens = store::load_secret(STORE)
            .as_ref()
            .and_then(Tokens::from_json);
        self.tokens.is_some()
    }
    fn begin_login(&mut self) -> Result<Box<dyn Login>, String> {
        Ok(Box::new(BrowserLogin::start()?))
    }
    fn accept_login(&mut self, creds: Value) -> Result<(), String> {
        let tokens = Tokens::from_json(&creds)
            .ok_or("ChatGPT sign-in response had no subscription credentials")?;
        store::save_secret(STORE, &tokens.to_json())?;
        self.tokens = Some(tokens);
        self.dirty = false;
        Ok(())
    }
    fn sign_out(&mut self) {
        store::delete_secret(STORE);
        self.tokens = None;
        self.dirty = false;
    }
    fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError> {
        let t = self.tokens.as_mut().ok_or(FetchError::SignedOut)?;
        if t.expires_ms.saturating_sub(300_000) <= time::now_ms() {
            refresh(t)?;
            self.dirty = true;
        }
        if self.dirty {
            store::save_secret(STORE, &t.to_json())?;
            self.dirty = false;
        }
        let mut r = usage(t)?;
        if r.status == 401 {
            refresh(t)?;
            self.dirty = true;
            store::save_secret(STORE, &t.to_json())?;
            self.dirty = false;
            r = usage(t)?;
            if r.status == 401 {
                return Err(FetchError::SignedOut);
            }
        }
        if r.status != 200 {
            return Err(FetchError::Other(format!(
                "ChatGPT usage request failed (HTTP {})",
                r.status
            )));
        }
        let snapshot = parse_usage(&r.json()?).ok_or_else(|| {
            FetchError::Other("ChatGPT returned no subscription usage windows".into())
        })?;
        send(snapshot);
        Ok(())
    }
}
fn usage(t: &Tokens) -> Result<http::Response, String> {
    http::get(
        USAGE_URL,
        &[
            AGENT,
            ("Authorization", &format!("Bearer {}", t.access)),
            ("ChatGPT-Account-Id", &t.account),
            ("Accept", "application/json"),
        ],
    )
}
fn refresh(t: &mut Tokens) -> Result<(), FetchError> {
    let r = http::post_json(
        TOKEN_URL,
        &[AGENT],
        &json!({"grant_type":"refresh_token","client_id":CLIENT_ID,"refresh_token":t.refresh}),
    )?;
    if matches!(r.status, 400 | 401) {
        return Err(FetchError::SignedOut);
    }
    if r.status != 200 {
        return Err(FetchError::Other(format!(
            "ChatGPT token refresh failed (HTTP {})",
            r.status
        )));
    }
    *t = Tokens::from_grant(&r.json()?, Some(t))
        .ok_or_else(|| FetchError::Other("ChatGPT token refresh response was incomplete".into()))?;
    Ok(())
}

struct BrowserLogin {
    listener: TcpListener,
    url: String,
    redirect: String,
    state: String,
    verifier: String,
    started: Instant,
}
impl BrowserLogin {
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 1455))
            .or_else(|_| TcpListener::bind(("127.0.0.1", 1457)))
            .map_err(|_| {
                "ChatGPT sign-in needs local port 1455 or 1457; both are unavailable".to_string()
            })?;
        Self::with_listener(listener)
    }
    fn with_listener(listener: TcpListener) -> Result<Self, String> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let redirect = format!(
            "http://127.0.0.1:{}/auth/callback",
            listener.local_addr().map_err(|e| e.to_string())?.port()
        );
        let state = crypto::b64url_encode(&crypto::random_bytes::<32>());
        let verifier = crypto::b64url_encode(&crypto::random_bytes::<32>());
        let challenge = crypto::b64url_encode(&crypto::sha256(verifier.as_bytes()));
        let url = format!(
            "https://auth.openai.com/oauth/authorize?{}",
            form(&[
                ("client_id", CLIENT_ID),
                ("response_type", "code"),
                ("redirect_uri", &redirect),
                ("scope", "openid profile email offline_access"),
                ("state", &state),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
                ("id_token_add_organizations", "true"),
                ("codex_cli_simplified_flow", "true"),
                ("originator", "ai_usage")
            ])
        );
        Ok(Self {
            listener,
            url,
            redirect,
            state,
            verifier,
            started: Instant::now(),
        })
    }
    fn handle(&self, stream: TcpStream) -> Option<PollResult> {
        self.handle_with(stream, |code| self.exchange(code))
    }
    fn handle_with(
        &self,
        mut stream: TcpStream,
        exchange: impl FnOnce(&str) -> PollResult,
    ) -> Option<PollResult> {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while request.len() < 8192 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let _ = stream.set_read_timeout(Some(remaining.min(Duration::from_millis(500))));
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => return None,
                Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
            if request.windows(4).any(|s| s == b"\r\n\r\n") {
                break;
            }
        }
        if !request.windows(4).any(|s| s == b"\r\n\r\n") {
            respond(&mut stream, 400, "Incomplete request");
            return None;
        }
        let request = String::from_utf8_lossy(&request);
        let target = request
            .lines()
            .next()?
            .strip_prefix("GET ")?
            .split_whitespace()
            .next()?;
        let Some(query) = target.strip_prefix("/auth/callback?") else {
            respond(&mut stream, 404, "Not found");
            return None;
        };
        let code = match callback(query, &self.state) {
            Ok(code) => code,
            Err(e) => {
                respond(&mut stream, 400, e);
                if e == "ChatGPT authorization was declined; cancel or retry sign-in" {
                    return Some(PollResult::Failed(
                        "ChatGPT authorization was declined".into(),
                    ));
                }
                return None;
            }
        };
        let result = exchange(&code);
        let message = if matches!(result, PollResult::Done(_)) {
            "ChatGPT sign-in complete. Return to TokenKeeper."
        } else {
            "ChatGPT sign-in failed. Try again from TokenKeeper."
        };
        respond(&mut stream, 200, message);
        Some(result)
    }
    fn exchange(&self, code: &str) -> PollResult {
        let body = form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("redirect_uri", &self.redirect),
            ("code_verifier", &self.verifier),
        ]);
        match http::request(
            "POST",
            TOKEN_URL,
            &[AGENT, ("Content-Type", "application/x-www-form-urlencoded")],
            Some(body.as_bytes()),
        ) {
            Ok(r) if r.status == 200 => match r
                .json()
                .ok()
                .as_ref()
                .and_then(|v| Tokens::from_grant(v, None))
            {
                Some(t) => PollResult::Done(t.to_json()),
                None => PollResult::Failed(
                    "ChatGPT sign-in response had no subscription credentials".into(),
                ),
            },
            Ok(r) => PollResult::Failed(format!("ChatGPT sign-in failed (HTTP {})", r.status)),
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
        if self.started.elapsed() > Duration::from_secs(10 * 60) {
            return PollResult::Failed("ChatGPT sign-in timed out".into());
        }
        // Bound work per poll so cancellation still works with unsolicited requests.
        for _ in 0..4 {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = self.handle(stream) {
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return PollResult::Failed("ChatGPT callback listener failed".into()),
            }
        }
        PollResult::Pending
    }
}
fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{message}",
        if status == 200 { "OK" } else { "Bad Request" },
        message.len()
    );
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}
fn decode(s: &str) -> Option<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}
fn callback(query: &str, state: &str) -> Result<String, &'static str> {
    let mut params = std::collections::HashMap::new();
    for field in query.split('&') {
        let (key, value) = field.split_once('=').ok_or("Malformed callback")?;
        let key = decode(key).ok_or("Malformed callback")?;
        let value = decode(value).ok_or("Malformed callback")?;
        if params.insert(key, value).is_some() {
            return Err("Duplicate callback parameter");
        }
    }
    if params.get("state").map(String::as_str) != Some(state) {
        return Err("Sign-in state mismatch");
    }
    if params.contains_key("error") {
        return Err("ChatGPT authorization was declined; cancel or retry sign-in");
    }
    params
        .remove("code")
        .filter(|c| !c.is_empty())
        .ok_or("Missing authorization code")
}
fn parse_usage(v: &Value) -> Option<Snapshot> {
    // Normalize the web API's seconds/snake_case into the common window parser.
    fn bucket(rate: &Value, plan: &Value) -> Value {
        let window = |key: &str| {
            let w = &rate[key];
            json!({"usedPercent":w["used_percent"],"windowDurationMins":w["limit_window_seconds"].as_i64().map(|s|s/60),"resetsAt":w["reset_at"]})
        };
        json!({"planType":plan,"primary":window("primary_window"),"secondary":window("secondary_window")})
    }
    let mut by_id = serde_json::Map::new();
    by_id.insert("codex".into(), bucket(&v["rate_limit"], &v["plan_type"]));
    for (i, limit) in v["additional_rate_limits"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = limit["limit_name"]
            .as_str()
            .or_else(|| limit["metered_feature"].as_str())
            .unwrap_or("Additional limit");
        by_id.insert(
            format!("{name} ({})", i + 1),
            bucket(&limit["rate_limit"], &v["plan_type"]),
        );
    }
    parse_limits(&json!({"rateLimitsByLimitId":by_id}))
}

fn parse_limits(v: &Value) -> Option<Snapshot> {
    let root = &v["rateLimits"];
    let entries: Vec<(&str, &Value)> = if let Some(by_id) = v["rateLimitsByLimitId"]
        .as_object()
        .filter(|x| !x.is_empty())
    {
        by_id.iter().map(|(k, v)| (k.as_str(), v)).collect()
    } else {
        vec![("codex", root)]
    };
    let mut windows = Vec::new();
    let mut plan = "ChatGPT".to_string();
    for (name, entry) in entries {
        if name.eq_ignore_ascii_case("codex") {
            if let Some(p) = entry["planType"]
                .as_str()
                .or_else(|| v["planType"].as_str())
            {
                plan = p.to_string();
            }
        }
        for key in ["primary", "secondary"] {
            let w = &entry[key];
            let Some(percent) = w["usedPercent"].as_f64().filter(|p| p.is_finite()) else {
                continue;
            };
            let Some(mins) = w["windowDurationMins"].as_i64().filter(|m| *m > 0) else {
                continue;
            };
            let Some(span) = mins.checked_mul(60_000) else {
                continue;
            };
            let label = if mins >= 7 * 24 * 60 {
                "Weekly".to_string()
            } else if mins >= 24 * 60 {
                format!("{}-day", mins / (24 * 60))
            } else if mins >= 60 {
                format!("{}-hour", mins / 60)
            } else {
                format!("{mins}-minute")
            };
            let label = if name.eq_ignore_ascii_case("codex") {
                label
            } else {
                format!("{name} {label}")
            };
            let reset = w["resetsAt"].as_i64().and_then(|s| s.checked_mul(1000));
            windows.push((
                name.eq_ignore_ascii_case("codex"),
                label,
                percent.clamp(0.0, 100.0),
                span,
                reset,
            ));
        }
    }
    if windows.is_empty() {
        return None;
    }
    // Prefer the Codex subscription bucket; within it, the longest reported
    // window is the headline (often weekly, but never assumed).
    let headline = windows
        .iter()
        .enumerate()
        .max_by_key(|(_, w)| (w.0, w.3))
        .map(|(i, _)| i)?;
    let (_, label, percent, span, reset) = windows[headline].clone();
    let now = time::now_ms();
    let mut snap = Snapshot::new(ProviderId::Codex, Unit::Basis);
    snap.plan = format!("{plan} · {label}");
    snap.limit = 10_000;
    snap.used = (percent * 100.0).round() as u32;
    snap.cycle_end_ms = reset.unwrap_or_else(|| now.saturating_add(span));
    snap.cycle_start_ms = snap.cycle_end_ms.saturating_sub(span);
    snap.meters = windows
        .into_iter()
        .map(|(_, label, p, _, resets_ms)| Meter {
            label,
            fraction: (p / 100.0) as f32,
            resets_ms,
        })
        .collect();
    snap.details = vec![Detail::Limits];
    snap.empty_note = "Rate limits unavailable";
    Some(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token(claims: Value) -> String {
        format!(
            "header.{}.signature",
            crypto::b64url_encode(claims.to_string().as_bytes())
        )
    }
    #[test]
    fn grant_reads_account_and_rotates_refresh_without_storing_id_token() {
        let id =
            token(json!({"https://api.openai.com/auth":{"chatgpt_account_id":"account-test"}}));
        let access = token(json!({"exp":2000000000}));
        let t = Tokens::from_grant(
            &json!({"id_token":id,"access_token":access,"refresh_token":"first"}),
            None,
        )
        .unwrap();
        assert_eq!(t.account, "account-test");
        assert_eq!(t.expires_ms, 2000000000000);
        assert!(t.to_json().get("id_token").is_none());
        let rotated = Tokens::from_grant(
            &json!({"access_token":"new","refresh_token":"rotated","expires_in":3600}),
            Some(&t),
        )
        .unwrap();
        assert_eq!(rotated.refresh, "rotated");
        assert_eq!(rotated.account, "account-test");
        let kept = Tokens::from_grant(
            &json!({"access_token":"new","expires_in":3600}),
            Some(&rotated),
        )
        .unwrap();
        assert_eq!(kept.refresh, "rotated");
    }
    #[test]
    fn web_usage_handles_weekly_primary_and_additional_buckets() {
        let s = parse_usage(&json!({"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":26,"limit_window_seconds":604800,"reset_at":1792000000},"secondary_window":null},"additional_rate_limits":[{"limit_name":"Review","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":300*60,"reset_at":1791000000}}}]})).unwrap();
        assert_eq!(s.used, 2600);
        assert_eq!(s.cycle_end_ms, 1792000000000);
        assert_eq!(s.meters.len(), 2);
        assert!(s.plan.contains("pro"));
    }
    #[test]
    fn callback_rejects_wrong_state_duplicates_and_malformed_encoding() {
        assert_eq!(
            callback("state=expected&code=a%2Bb", "expected").unwrap(),
            "a+b"
        );
        assert!(callback("state=wrong&code=secret", "expected").is_err());
        assert!(callback("state=expected&state=expected&code=x", "expected").is_err());
        assert!(callback("state=expected&code=%ZZ", "expected").is_err());
        assert!(callback("state=expected&error=access_denied", "expected").is_err());
    }
    #[test]
    fn loopback_callback_exchanges_only_matching_state() {
        let login =
            BrowserLogin::with_listener(TcpListener::bind(("127.0.0.1", 0)).unwrap()).unwrap();
        assert!(login.url.contains("code_challenge_method=S256"));
        assert!(
            login
                .url
                .contains("scope=openid%20profile%20email%20offline_access")
        );
        let mut client = TcpStream::connect(login.listener.local_addr().unwrap()).unwrap();
        write!(
            client,
            "GET /auth/callback?state={}&code=a%2Bb HTTP/1.1\r\nHost: localhost\r\n\r\n",
            login.state
        )
        .unwrap();
        let (stream, _) = login.listener.accept().unwrap();
        let result = login.handle_with(stream, |code| {
            assert_eq!(code, "a+b");
            PollResult::Done(json!({"access":"test"}))
        });
        assert!(matches!(result, Some(PollResult::Done(_))));
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        let mut client = TcpStream::connect(login.listener.local_addr().unwrap()).unwrap();
        client
            .write_all(
                b"GET /auth/callback?state=wrong&code=secret HTTP/1.1\r\nHost: localhost\r\n\r\n",
            )
            .unwrap();
        let (stream, _) = login.listener.accept().unwrap();
        assert!(
            login
                .handle_with(stream, |_| panic!(
                    "must not exchange a mismatched callback"
                ))
                .is_none()
        );
    }
    #[test]
    fn rejects_empty_and_invalid_windows() {
        assert!(
            parse_limits(&json!({"rateLimits": {"primary": null, "secondary": null}})).is_none()
        );
        assert!(
            parse_limits(
                &json!({"rateLimits": {"primary": {"usedPercent": 12, "windowDurationMins": -5}}})
            )
            .is_none()
        );
        assert!(parse_limits(&json!({"rateLimits": {"primary": {"usedPercent": 12, "windowDurationMins": 9223372036854775807i64}}})).is_none());
    }

    #[test]
    fn prefers_codex_weekly_over_longer_other_bucket() {
        let v = json!({"rateLimitsByLimitId": {
            "other": {"primary": {"usedPercent": 95, "windowDurationMins": 20160, "resetsAt": 1792000000}},
            "codex": {"planType": "plus", "primary": {"usedPercent": 15, "windowDurationMins": 300, "resetsAt": 1791000000},
                      "secondary": {"usedPercent": 60, "windowDurationMins": 10080, "resetsAt": 1792000000}}
        }});
        let s = parse_limits(&v).unwrap();
        assert_eq!(s.used, 6000);
        assert_eq!(s.meters.len(), 3);
        assert!(s.plan.contains("plus · Weekly"));
    }

    #[test]
    fn parses_legacy_single_bucket() {
        let s = parse_limits(&json!({"rateLimits": {"primary": {"usedPercent": 10, "windowDurationMins": 300, "resetsAt": 1792000000}}})).unwrap();
        assert_eq!(s.used, 1000);
        assert_eq!(s.meters[0].label, "5-hour");
    }

    #[test]
    fn parses_weekly_without_assuming_session() {
        let v = json!({"rateLimitsByLimitId":{"codex":{"planType":"pro",
            "primary":{"usedPercent":25.5,"windowDurationMins":10080,"resetsAt":1792000000},
            "secondary":null
        }}});
        let s = parse_limits(&v).unwrap();
        assert_eq!(s.used, 2550);
        assert_eq!(s.details, vec![Detail::Limits]);
        assert_eq!(s.meters.len(), 1);
        assert!(s.meters[0].label.contains("Weekly"));
    }
}
