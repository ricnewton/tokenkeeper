//! OpenCode Go via Console browser device authorization. No supplier files or CLI.
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::model::{Detail, Meter, ProviderId, Snapshot, Unit};
use crate::provider::{FetchError, Login, PollResult, Provider};
use crate::{http, store, time};

const CONSOLE: &str = "https://opencode.ai/console";
const TOKEN_URL: &str = "https://opencode.ai/console/auth/device/token";
// The built-in Console client, as used by upstream's provider plugin. This is
// an HTTP protocol identifier; TokenKeeper never invokes that executable.
const CLIENT_ID: &str = "opencode-cli";
const STORE: &str = "opencode-go";
const AGENT: (&str, &str) = (
    "User-Agent",
    concat!("tokenkeeper/", env!("CARGO_PKG_VERSION")),
);

#[derive(Clone)]
struct Tokens {
    access: String,
    refresh: String,
    org: Option<String>,
    expires_ms: i64,
}

fn header_value(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_graphic())
}

impl Tokens {
    fn from_json(v: &Value) -> Option<Self> {
        let access = v["access"].as_str()?.to_owned();
        let refresh = v["refresh"].as_str()?.to_owned();
        let org = v["org"].as_str().map(String::from);
        if !header_value(&access)
            || !header_value(&refresh)
            || org.as_deref().is_some_and(|o| !header_value(o))
        {
            return None;
        }
        Some(Self {
            access,
            refresh,
            org,
            expires_ms: v["expires_ms"].as_i64()?,
        })
    }

    fn to_json(&self) -> Value {
        json!({"access":self.access,"refresh":self.refresh,"org":self.org,"expires_ms":self.expires_ms})
    }

    fn from_grant(v: &Value, old: Option<&Self>) -> Option<Self> {
        let expires_ms =
            time::now_ms().checked_add(v["expires_in"].as_i64()?.checked_mul(1000)?)?;
        if expires_ms <= time::now_ms() {
            return None;
        }
        let refresh = v["refresh_token"]
            .as_str()
            .or_else(|| old.map(|t| t.refresh.as_str()))?;
        let org = v["org_id"]
            .as_str()
            .or_else(|| old.and_then(|t| t.org.as_deref()));
        Self::from_json(
            &json!({"access":v["access_token"],"refresh":refresh,"org":org,"expires_ms":expires_ms}),
        )
    }
}

#[derive(Default)]
pub struct OpenCode {
    tokens: Option<Tokens>,
}

// A small boundary lets tests exercise refresh and secure-write ordering without
// contacting an account or touching the user's credential store.
trait SessionIo {
    fn refresh(&mut self, body: &Value) -> Result<http::Response, String>;
    fn get(
        &mut self,
        path: &str,
        tokens: &Tokens,
        org: Option<&str>,
    ) -> Result<http::Response, String>;
    fn save(&mut self, value: &Value) -> Result<(), String>;
}
struct ConsoleIo;
impl SessionIo for ConsoleIo {
    fn refresh(&mut self, body: &Value) -> Result<http::Response, String> {
        post(TOKEN_URL, body)
    }
    fn get(
        &mut self,
        path: &str,
        tokens: &Tokens,
        org: Option<&str>,
    ) -> Result<http::Response, String> {
        get(path, tokens, org)
    }
    fn save(&mut self, value: &Value) -> Result<(), String> {
        store::save_secret(STORE, value)
    }
}

impl OpenCode {
    fn refresh(&mut self, force: bool, io: &mut impl SessionIo) -> Result<(), FetchError> {
        let old = self.tokens.as_ref().ok_or(FetchError::SignedOut)?;
        if !force && old.expires_ms.saturating_sub(5 * 60 * 1000) > time::now_ms() {
            return Ok(());
        }
        let r = io.refresh(
            &json!({"client_id":CLIENT_ID,"grant_type":"refresh_token","refresh_token":old.refresh}),
        )?;
        if r.status == 401 {
            return Err(FetchError::SignedOut);
        }
        let v = r
            .json()
            .map_err(|_| FetchError::Other("OpenCode returned an invalid token response".into()))?;
        if v["error"] == "invalid_grant" {
            return Err(FetchError::SignedOut);
        }
        if r.status != 200 {
            return Err(request_error("Token refresh", r.status));
        }
        let fresh = Tokens::from_grant(&v, Some(old)).ok_or_else(|| {
            FetchError::Other("OpenCode returned an incomplete token response".into())
        })?;
        // Rotation must be saved before any further requests. Keep the new token
        // in memory even if storage fails, so a retry never uses the old refresh.
        self.tokens = Some(fresh);
        self.save(io)
    }

    fn save(&self, io: &mut impl SessionIo) -> Result<(), FetchError> {
        io.save(&self.tokens.as_ref().ok_or(FetchError::SignedOut)?.to_json())
            .map_err(|_| FetchError::Other("Could not save the OpenCode session securely".into()))
    }

    fn get(
        &mut self,
        path: &str,
        scoped: bool,
        io: &mut impl SessionIo,
    ) -> Result<Value, FetchError> {
        self.refresh(false, io)?;
        let tokens = self.tokens.as_ref().ok_or(FetchError::SignedOut)?;
        let mut r = io.get(
            path,
            tokens,
            if scoped { tokens.org.as_deref() } else { None },
        )?;
        if r.status == 401 {
            self.refresh(true, io)?;
            let tokens = self.tokens.as_ref().ok_or(FetchError::SignedOut)?;
            r = io.get(
                path,
                tokens,
                if scoped { tokens.org.as_deref() } else { None },
            )?;
        }
        if r.status == 401 {
            return Err(FetchError::SignedOut);
        }
        if r.status != 200 {
            return Err(request_error("Usage request", r.status));
        }
        r.json()
            .map_err(|_| FetchError::Other("OpenCode returned an invalid Console response".into()))
    }

    fn fetch_with(
        &mut self,
        send: &mut dyn FnMut(Snapshot),
        io: &mut impl SessionIo,
    ) -> Result<(), FetchError> {
        // Retry a previously failed secure write before using a rotated token.
        self.save(io)?;
        self.refresh(false, io)?;
        if self
            .tokens
            .as_ref()
            .ok_or(FetchError::SignedOut)?
            .org
            .is_none()
        {
            let orgs = self.get("/api/orgs", false, io)?;
            // A 401 retry during discovery may have returned an org-scoped
            // grant. Only fall back to the list when no scope was granted.
            if self
                .tokens
                .as_ref()
                .ok_or(FetchError::SignedOut)?
                .org
                .is_none()
            {
                let org = select_org(&orgs).map_err(FetchError::Other)?;
                self.tokens.as_mut().ok_or(FetchError::SignedOut)?.org = Some(org);
                self.save(io)?;
            }
        }
        let v = self.get("/api/go/status", true, io)?;
        send(parse_usage(&v).map_err(FetchError::Other)?);
        Ok(())
    }
}

impl Provider for OpenCode {
    fn id(&self) -> ProviderId {
        ProviderId::OpenCode
    }
    fn load(&mut self) -> bool {
        self.tokens = store::load_secret(STORE)
            .as_ref()
            .and_then(Tokens::from_json);
        self.tokens.is_some()
    }
    fn begin_login(&mut self) -> Result<Box<dyn Login>, String> {
        let r = post(
            &format!("{CONSOLE}/auth/device/code"),
            &json!({"client_id":CLIENT_ID,"supports_org_scope":true}),
        )?;
        if r.status != 200 {
            return Err(format!(
                "OpenCode browser sign-in failed (HTTP {})",
                r.status
            ));
        }
        let v = r
            .json()
            .map_err(|_| "OpenCode returned an invalid sign-in response")?;
        Ok(Box::new(DeviceLogin::from_json(&v)?))
    }
    fn accept_login(&mut self, creds: Value) -> Result<(), String> {
        let tokens = Tokens::from_json(&creds).ok_or("OpenCode sign-in response was incomplete")?;
        store::save_secret(STORE, &tokens.to_json())?;
        self.tokens = Some(tokens);
        Ok(())
    }
    fn sign_out(&mut self) {
        store::delete_secret(STORE);
        self.tokens = None;
    }
    fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError> {
        self.fetch_with(send, &mut ConsoleIo)
    }
}

fn post(url: &str, body: &Value) -> Result<http::Response, String> {
    http::post_json(url, &[AGENT, ("Accept", "application/json")], body)
}
fn get(path: &str, tokens: &Tokens, org: Option<&str>) -> Result<http::Response, String> {
    let auth = format!("Bearer {}", tokens.access);
    let mut headers = vec![
        AGENT,
        ("Accept", "application/json"),
        ("Authorization", auth.as_str()),
    ];
    if let Some(org) = org {
        headers.push(("x-org-id", org));
    }
    http::get(&format!("{CONSOLE}{path}"), &headers)
}
fn request_error(context: &str, status: u32) -> FetchError {
    let message = if status == 403 {
        "OpenCode Console denied Go usage access. Check the selected workspace and subscription"
            .into()
    } else {
        format!("OpenCode {context} failed (HTTP {status})")
    };
    FetchError::Other(message)
}

fn select_org(v: &Value) -> Result<String, String> {
    let orgs = v
        .as_array()
        .ok_or("OpenCode returned an unrecognised workspace list")?;
    if orgs.len() != 1 {
        return Err(
            "Choose the workspace with your Go subscription during browser sign-in, then reconnect"
                .into(),
        );
    }
    orgs[0]["id"]
        .as_str()
        .filter(|id| header_value(id))
        .map(String::from)
        .ok_or_else(|| "OpenCode workspace response had no valid ID".into())
}

struct DeviceLogin {
    device: String,
    code: String,
    url: String,
    interval: Duration,
    expires: Instant,
}

fn verification_url(value: &str) -> Option<String> {
    let url = if value.starts_with('/') && !value.starts_with("//") {
        format!("https://opencode.ai{value}")
    } else {
        value.to_owned()
    };
    // Never open a provider-returned URL on another origin or accept embedded
    // credentials/control characters. The Console's own approval page is HTTPS.
    if !url.starts_with("https://opencode.ai/")
        || url.bytes().any(|b| b.is_ascii_control() || b == b'\\')
    {
        return None;
    }
    Some(url)
}

impl DeviceLogin {
    fn from_json(v: &Value) -> Result<Self, String> {
        let field = |name: &str| {
            v[name]
                .as_str()
                .filter(|s| header_value(s))
                .map(String::from)
                .ok_or_else(|| "OpenCode sign-in response was incomplete".to_owned())
        };
        let url = v["verification_uri_complete"]
            .as_str()
            .and_then(verification_url)
            .ok_or("OpenCode returned an unsupported browser approval URL")?;
        let expires = v["expires_in"]
            .as_u64()
            .filter(|s| *s > 0 && *s <= 3600)
            .ok_or("OpenCode returned an invalid sign-in expiry")?;
        Ok(Self {
            device: field("device_code")?,
            code: field("user_code")?,
            url,
            interval: Duration::from_secs(v["interval"].as_u64().unwrap_or(5).clamp(1, 60)),
            expires: Instant::now() + Duration::from_secs(expires),
        })
    }

    fn response(&mut self, status: u32, v: &Value) -> PollResult {
        if status == 200 {
            if let Some(tokens) = Tokens::from_grant(v, None) {
                return PollResult::Done(tokens.to_json());
            }
        }
        match v["error"].as_str() {
            Some("authorization_pending") => PollResult::Pending,
            Some("slow_down") => {
                self.interval =
                    (self.interval + Duration::from_secs(5)).min(Duration::from_secs(60));
                PollResult::Pending
            }
            Some("access_denied") => PollResult::Failed("OpenCode sign-in was declined".into()),
            Some("expired_token") => PollResult::Failed("OpenCode sign-in code expired".into()),
            _ if status == 429 || status >= 500 => PollResult::Pending,
            _ => PollResult::Failed(format!("OpenCode authorization failed (HTTP {status})")),
        }
    }
}
impl Login for DeviceLogin {
    fn url(&self) -> &str {
        &self.url
    }
    fn user_code(&self) -> Option<&str> {
        Some(&self.code)
    }
    fn poll_interval(&self) -> Duration {
        self.interval
    }
    fn poll(&mut self) -> PollResult {
        if Instant::now() >= self.expires {
            return PollResult::Failed("OpenCode sign-in code expired".into());
        }
        let r = match post(
            TOKEN_URL,
            &json!({"client_id":CLIENT_ID,"device_code":self.device,"grant_type":"urn:ietf:params:oauth:grant-type:device_code"}),
        ) {
            Ok(r) => r,
            Err(_) => return PollResult::Pending,
        };
        match r.json() {
            Ok(v) => self.response(r.status, &v),
            Err(_) if r.status == 429 || r.status >= 500 => PollResult::Pending,
            Err(_) => PollResult::Failed(format!(
                "OpenCode returned an invalid authorization response (HTTP {})",
                r.status
            )),
        }
    }
}

fn parse_usage(v: &Value) -> Result<Snapshot, String> {
    if v.is_null() || v.get("access").is_some_and(Value::is_null) {
        return Err("No OpenCode Go subscription was reported for this workspace".into());
    }
    let access = &v["access"];
    let mut s = Snapshot::new(ProviderId::OpenCode, Unit::Basis);
    s.plan = "Go".into();
    for (key, label) in [
        ("fiveHour", "5-hour session"),
        ("week", "Weekly"),
        ("month", "Monthly"),
    ] {
        let meter = &access["meters"][key];
        if meter.is_null() {
            continue;
        }
        let number = |name: &str| {
            meter[name]
                .as_f64()
                .or_else(|| meter[name].as_str()?.parse::<f64>().ok())
        };
        let used = number("usedMicroCents").filter(|n| n.is_finite() && *n >= 0.0);
        let limit = number("limitMicroCents").filter(|n| n.is_finite() && *n > 0.0);
        let (Some(used), Some(limit)) = (used, limit) else {
            return Err("OpenCode returned an invalid Go usage meter".into());
        };
        let fraction = (used / limit) as f32;
        if !fraction.is_finite() {
            return Err("OpenCode returned an invalid Go usage percentage".into());
        }
        let resets = meter["resetsAt"]
            .as_str()
            .and_then(time::parse_timestamp_ms)
            .or_else(|| {
                (key == "month")
                    .then(|| access["endsAt"].as_str().and_then(time::parse_timestamp_ms))
                    .flatten()
            });
        s.meters.push(Meter {
            label: label.into(),
            fraction,
            resets_ms: resets,
        });
    }
    let headline = s
        .meters
        .last()
        .ok_or("OpenCode returned no Go usage meters")?;
    s.limit = 10_000;
    s.used = (headline.fraction * 10_000.0).round() as u32;
    s.cycle_end_ms = headline.resets_ms.unwrap_or(0);
    // The service does not report the start of the quota window. Do not invent
    // one or derive pace/history from this reading.
    s.plan = format!("Go · {}", headline.label);
    s.details = vec![Detail::Limits];
    s.empty_note = "OpenCode reports subscription limits only";
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn response(status: u32, value: Value) -> http::Response {
        http::Response {
            status,
            body: value.to_string().into_bytes(),
        }
    }
    fn quota() -> Value {
        json!({"access":{"meters":{"month":{"usedMicroCents":"20","limitMicroCents":"100"}}}})
    }
    fn session(expired: bool) -> OpenCode {
        let mut tokens = Tokens::from_grant(&json!({"access_token":"old-access","refresh_token":"old-refresh","expires_in":3600,"org_id":"old-org"}), None).unwrap();
        if expired {
            tokens.expires_ms = 1;
        }
        OpenCode {
            tokens: Some(tokens),
        }
    }
    struct FakeIo {
        reads: VecDeque<http::Response>,
        refresh: Option<Result<http::Response, String>>,
        log: Vec<&'static str>,
        refresh_keys: Vec<String>,
        scopes: Vec<Option<String>>,
        access: Vec<String>,
        saved: Vec<Value>,
        fail_save: Option<usize>,
    }
    impl FakeIo {
        fn new(reads: Vec<http::Response>) -> Self {
            Self {
                reads: reads.into(),
                refresh: Some(Ok(response(
                    200,
                    json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"org_id":"new-org"}),
                ))),
                log: vec![],
                refresh_keys: vec![],
                scopes: vec![],
                access: vec![],
                saved: vec![],
                fail_save: None,
            }
        }
    }
    impl SessionIo for FakeIo {
        fn refresh(&mut self, body: &Value) -> Result<http::Response, String> {
            self.log.push("refresh");
            self.refresh_keys
                .push(body["refresh_token"].as_str().unwrap().into());
            self.refresh.take().expect("refreshed more than once")
        }
        fn get(
            &mut self,
            _: &str,
            tokens: &Tokens,
            org: Option<&str>,
        ) -> Result<http::Response, String> {
            self.log.push("get");
            self.scopes.push(org.map(String::from));
            self.access.push(tokens.access.clone());
            Ok(self.reads.pop_front().expect("unexpected request"))
        }
        fn save(&mut self, value: &Value) -> Result<(), String> {
            self.log.push("save");
            self.saved.push(value.clone());
            if self.fail_save == Some(self.saved.len()) {
                return Err("storage unavailable".into());
            }
            Ok(())
        }
    }

    #[test]
    fn saves_rotated_tokens_before_usage_and_uses_updated_org() {
        let mut p = session(true);
        let mut io = FakeIo::new(vec![response(200, quota())]);
        let mut snap = None;
        assert!(p.fetch_with(&mut |s| snap = Some(s), &mut io).is_ok());
        assert_eq!(io.log, ["save", "refresh", "save", "get"]);
        assert_eq!(io.saved[1]["refresh"], "new-refresh");
        assert_eq!(io.scopes, [Some("new-org".into())]);
        assert_eq!(io.access, ["new-access"]);
        assert_eq!(snap.unwrap().used, 2000);
    }
    #[test]
    fn failed_secure_write_stops_usage_and_retries_new_tokens() {
        let mut p = session(true);
        let mut io = FakeIo::new(vec![response(200, quota())]);
        io.fail_save = Some(2);
        let mut published = false;
        assert!(matches!(
            p.fetch_with(&mut |_| published = true, &mut io),
            Err(FetchError::Other(_))
        ));
        assert!(!published);
        assert!(io.scopes.is_empty());
        assert_eq!(p.tokens.as_ref().unwrap().refresh, "new-refresh");
        assert!(p.fetch_with(&mut |_| published = true, &mut io).is_ok());
        assert!(published);
        assert_eq!(io.refresh_keys, ["old-refresh"]);
        assert_eq!(io.saved[2]["refresh"], "new-refresh");
    }
    #[test]
    fn unauthorized_usage_refreshes_once_and_recomputes_scope() {
        let mut p = session(false);
        let mut io = FakeIo::new(vec![response(401, json!({})), response(200, quota())]);
        assert!(p.fetch_with(&mut |_| {}, &mut io).is_ok());
        assert_eq!(io.log, ["save", "get", "refresh", "save", "get"]);
        assert_eq!(io.access, ["old-access", "new-access"]);
        assert_eq!(io.scopes, [Some("old-org".into()), Some("new-org".into())]);
        let mut io = FakeIo::new(vec![response(401, json!({})), response(401, json!({}))]);
        assert!(matches!(
            session(false).fetch_with(&mut |_| panic!("published a failed read"), &mut io),
            Err(FetchError::SignedOut)
        ));
        assert_eq!(io.refresh_keys.len(), 1);
    }
    #[test]
    fn refresh_rejection_ends_session_but_transient_errors_do_not() {
        for r in [
            response(400, json!({"error":"invalid_grant"})),
            http::Response {
                status: 401,
                body: vec![],
            },
        ] {
            let mut io = FakeIo::new(vec![]);
            io.refresh = Some(Ok(r));
            assert!(matches!(
                session(true).fetch_with(&mut |_| {}, &mut io),
                Err(FetchError::SignedOut)
            ));
        }
        for r in [
            Err("network unavailable".into()),
            Ok(response(503, json!({}))),
        ] {
            let mut p = session(true);
            let mut io = FakeIo::new(vec![]);
            io.refresh = Some(r);
            assert!(matches!(
                p.fetch_with(&mut |_| {}, &mut io),
                Err(FetchError::Other(_))
            ));
            assert_eq!(p.tokens.as_ref().unwrap().refresh, "old-refresh");
        }
        let mut io = FakeIo::new(vec![response(403, json!({}))]);
        assert!(matches!(
            session(false).fetch_with(&mut |_| {}, &mut io),
            Err(FetchError::Other(_))
        ));
        assert!(io.refresh_keys.is_empty());
    }
    fn device() -> DeviceLogin {
        DeviceLogin::from_json(&json!({"device_code":"secret-device","user_code":"ABCD-EFGH","verification_uri_complete":"https://opencode.ai/console/auth/device?code=ABCD-EFGH","expires_in":600,"interval":5})).unwrap()
    }
    #[test]
    fn handles_device_pending_slowdown_and_decline() {
        let mut d = device();
        assert!(matches!(
            d.response(400, &json!({"error":"authorization_pending"})),
            PollResult::Pending
        ));
        assert!(matches!(
            d.response(400, &json!({"error":"slow_down"})),
            PollResult::Pending
        ));
        assert_eq!(d.poll_interval(), Duration::from_secs(10));
        assert!(matches!(
            d.response(400, &json!({"error":"access_denied"})),
            PollResult::Failed(_)
        ));
        assert!(matches!(d.response(200, &json!({"access_token":"access","refresh_token":"refresh","expires_in":3600,"org_id":"org_1"})), PollResult::Done(_)));
        assert!(matches!(d.response(200, &json!({})), PollResult::Failed(_)));
        assert!(matches!(d.response(500, &json!({})), PollResult::Pending));
    }
    #[test]
    fn validates_approval_origin_and_expiry() {
        for url in [
            "http://opencode.ai/auth",
            "https://opencode.ai.evil/auth",
            "https://opencode.ai@evil/auth",
            "//evil/auth",
            "https://opencode.ai/\\evil",
            "https://opencode.ai/\n",
        ] {
            assert!(verification_url(url).is_none(), "{url:?}");
        }
        assert_eq!(
            verification_url("/console/auth/device"),
            Some("https://opencode.ai/console/auth/device".into())
        );
        let mut d = device();
        d.expires = Instant::now();
        assert!(matches!(d.poll(), PollResult::Failed(_)));
    }
    #[test]
    fn rotates_tokens_and_preserves_org_and_optional_refresh() {
        let old = Tokens::from_grant(
            &json!({"access_token":"a","refresh_token":"r","expires_in":3600,"org_id":"org_1"}),
            None,
        )
        .unwrap();
        let fresh = Tokens::from_grant(
            &json!({"access_token":"b","refresh_token":"s","expires_in":3600}),
            Some(&old),
        )
        .unwrap();
        assert_eq!(
            (&*fresh.access, &*fresh.refresh, fresh.org.as_deref()),
            ("b", "s", Some("org_1"))
        );
        assert_eq!(Tokens::from_json(&fresh.to_json()).unwrap().org, fresh.org);
        assert!(
            Tokens::from_grant(
                &json!({"access_token":"a","refresh_token":"r","expires_in":-1}),
                None
            )
            .is_none()
        );
        assert!(
            Tokens::from_grant(
                &json!({"access_token":"a\n","refresh_token":"r","expires_in":3600}),
                None
            )
            .is_none()
        );
    }
    #[test]
    fn never_chooses_an_arbitrary_workspace() {
        assert_eq!(select_org(&json!([{"id":"org_1"}])).unwrap(), "org_1");
        assert!(select_org(&json!([{"id":"org_1"},{"id":"org_2"}])).is_err());
        assert!(select_org(&json!([])).is_err());
        assert!(select_org(&json!([{"id":"bad\n"}])).is_err());
    }
    #[test]
    fn parses_console_meters_without_inventing_pace_or_spending() {
        let s = parse_usage(&json!({"access":{"endsAt":"2026-11-01T00:00:00Z","meters":{
            "fiveHour":{"usedMicroCents":"300000000","limitMicroCents":"1200000000","resetsAt":"2026-10-11T18:00:00Z"},
            "week":{"usedMicroCents":1200000000u64,"limitMicroCents":3000000000u64},
            "month":{"usedMicroCents":"600000000","limitMicroCents":"6000000000","resetsAt":null}
        }}})).unwrap();
        assert_eq!(s.meters.len(), 3);
        assert_eq!(s.meters[0].fraction, 0.25);
        assert_eq!(s.meters[1].resets_ms, None);
        assert_eq!(s.used, 1000);
        assert_eq!(s.cycle_start_ms, 0);
        assert_eq!(
            s.cycle_end_ms,
            time::parse_timestamp_ms("2026-11-01T00:00:00Z").unwrap()
        );
        assert_eq!(s.details, vec![Detail::Limits]);
        assert!(s.history.is_empty());
    }
    #[test]
    fn missing_entitlement_and_malformed_usage_are_errors() {
        for v in [
            json!(null),
            json!({"access":null}),
            json!({}),
            json!({"access":{"meters":{"month":{"usedMicroCents":"NaN","limitMicroCents":"0"}}}}),
        ] {
            assert!(parse_usage(&v).is_err());
        }
        assert!(matches!(request_error("Usage", 403), FetchError::Other(_)));
    }
}
