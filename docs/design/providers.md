# Providers

The providers use **undocumented** endpoints, the same ones their own
editors and CLIs call. Parsing is deliberately defensive: unknown fields are
ignored, and missing ones fall back. When a response doesn't parse, the body
is saved to `%APPDATA%\ai-usage\debug\` so the parser can be fixed from real
data.

Each provider refreshes its access token when it is within 5 minutes of
expiry. If a request still gets a 401, it force-refreshes once and retries.
Only a second rejection becomes `SignedOut`.

## Cursor (`cursor.rs`)

**Sign-in (browser, PKCE-style polling):**
1. Generate a verifier and a UUID, then open `https://cursor.com/loginDeepControl?challenge=…&uuid=…&mode=login&redirectTarget=cli`.
2. Poll `https://api2.cursor.sh/auth/poll?uuid=…&verifier=…` every 2 s, for up to 5 minutes, until it returns `accessToken` and `refreshToken`.
3. Refresh through `https://api2.cursor.sh/oauth/token` (client_id `KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB`). A response with `shouldLogout: true` means the session is gone.

**Usage:** the dashboard APIs, authenticated by the cookie
`WorkosCursorSessionToken={userId}%3A%3A{accessToken}`. The `userId` is the JWT
`sub` after the `|`.
- `GET https://cursor.com/api/usage-summary` gives the plan, cycle dates, and used and included amounts in cents.
- `POST https://cursor.com/api/dashboard/get-filtered-usage-events` (pages of 100, at most 300 pages) gives per-request `chargedCents`. These events drive the Today, This cycle and History tabs. Past cycles are fetched once and cached.

**Gotchas:** cookie-authenticated requests need `Referer: https://cursor.com/dashboard`, and POSTs also need `Origin`. The default browser-style User-Agent is used here.

## GitHub Copilot (`copilot.rs`)

**Sign-in (GitHub device flow):**
- The client ID is `Iv1.b507a08c87ecfe98`, the Copilot editor app. The `copilot_internal` endpoints only accept tokens from Copilot clients.
- The scope is `read:user`.
- `POST https://github.com/login/device/code` returns `user_code` and `verification_uri`. The UI shows the code and copies it to the clipboard.
- Poll `POST https://github.com/login/oauth/access_token` with grant `urn:ietf:params:oauth:grant-type:device_code`. Handle `authorization_pending`, `slow_down` (add 5 s), `access_denied` and `expired_token`.
- GitHub-app tokens can expire. When a `refresh_token` and `expires_in` are returned, refresh with `grant_type=refresh_token`.

**Usage:** `GET https://api.github.com/copilot_internal/user`, with headers
`Authorization: token …` and `X-GitHub-Api-Version: 2025-04-01`.
- The allowance comes from `quota_snapshots.premium_interactions`: `entitlement`, `credits_used` or `entitlement - quota_remaining`, and `unlimited`. Credits are shown as dollars at 100 credits = $1 (unit `Cents`, one credit per cent), so a 10,000-credit seat reads as $100.00.
- The plan comes from `copilot_plan` (for example `enterprise` gives "Enterprise").
- The cycle ends at `quota_reset_date_utc`, a date-only value. The cycle start is one month before that.
- The unit is `Cents` (one credit per cent). The panel tabs are Today, This cycle and History, built by `tracker.rs`.

## Claude Code (`claude.rs`)

**Sign-in (OAuth 2 + PKCE, localhost redirect), the same flow Claude Code uses:**
- Open `https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&redirect_uri=http://localhost:{port}/callback&scope=user:profile&code_challenge=…&code_challenge_method=S256&state=…`.
- A non-blocking `TcpListener` on `127.0.0.1:{random port}` catches `/callback`. It checks `state`, answers with a small HTML page, then exchanges the code.
- Token exchange and refresh: `POST https://platform.claude.com/v1/oauth/token`, JSON body `{grant_type, code, redirect_uri, client_id, code_verifier, state}`.
- **Must not use a browser-like User-Agent.** With `Mozilla/5.0 …` Anthropic answers every token request with HTTP 429 `rate_limit_error`, even when the request is otherwise valid. All Claude requests send `User-Agent: ai-usage/<version>` (`AGENT` in `claude.rs`).

**Usage:** `GET https://api.anthropic.com/api/oauth/usage`, with headers
`Authorization: Bearer …` and `anthropic-beta: oauth-2025-04-20`. This is the
endpoint behind Claude Code's `/usage`. The plan comes from `/api/oauth/profile`
(`organization.organization_type`), fetched once.

Response shape and how it is read:
- `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`: `{utilization: %, resets_at}`, or `null`.
  - Primary meter: `seven_day`, falling back to `five_hour`.
  - All non-null windows become `meters`, with the 5-hour session first. Unit is `Basis`.
- `limits[]` entries of `kind: "weekly_scoped"` become extra "Weekly, {model}" meters, unless a named window already covers that model.
- When `extra_usage.is_enabled` is true, it also becomes an "Extra usage this month" meter.
- If every window is `null`, the provider falls back to `extra_usage`: `{is_enabled, monthly_limit, used_credits, currency, decimal_places}`. Amounts are in minor units, so `60000` means $600.00. Unit is `Cents`, and the cycle is the local calendar month.
- There are also many other keys with codenames (`tangelo`, `amber_gauge`, …), all `null` so far, plus `limits: []` and a `spend` block (`used.amount_minor`, `limit.amount_minor`, `percent`).

**Real Enterprise response (observed 6 Oct 2026):** all rate-limit windows are
`null`. The only data is the monthly spend cap:

```json
"extra_usage": {"is_enabled": true, "monthly_limit": 60000, "used_credits": 0.0, "currency": "USD", "decimal_places": 2, ...},
"spend": {"used": {"amount_minor": 0, ...}, "limit": {"amount_minor": 60000, ...}, "percent": 0, "severity": "normal", ...}
```

This means Enterprise seats show as **$X of $600.00** per month. It still needs
checking once some credits are used:
- Does `used_credits` count in minor units, as assumed, or in dollars?
- Does `spend.used.amount_minor` agree with it?
- What are the cycle dates? The response has none, so the cycle is currently assumed to be the local calendar month.
- Is the "Extra usage this month" meter redundant when it is also the headline ring? It probably should be hidden in that case.

## ChatGPT Codex (`codex.rs`)

Use supplier web APIs directly; no installed Codex executable or auth file is used. The browser OAuth flow follows OpenAI's [login source](https://github.com/openai/codex/blob/main/codex-rs/login/src/server.rs) and [OAuth client](https://github.com/openai/codex/blob/main/codex-rs/login/src/oauth/client.rs). It binds loopback port 1455, falling back to registered port 1457, generates PKCE S256 and random state, and opens `https://auth.openai.com/oauth/authorize`. Code exchange is form-encoded to `/oauth/token`; refresh uses JSON at that endpoint. The app requests `openid profile email offline_access`, without connector scopes. Callback state is checked before exchange; tokens and codes never go into debug dumps.

The app stores its own access/refresh token, expiry and account ID in DPAPI or Secret Service. Account ID comes from the token's `https://api.openai.com/auth.chatgpt_account_id` claim. Refresh token rotation is saved before usage polling continues. Signing out deletes only this app's credentials.

Usage is `GET https://chatgpt.com/backend-api/wham/usage` with bearer authorization and `ChatGPT-Account-Id`, as used in the [OpenAI backend client](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client.rs). Parse `plan_type`, nullable `rate_limit.primary_window`/`secondary_window`, and `additional_rate_limits`: `used_percent`, `limit_window_seconds`, `reset_at` (Unix seconds). Show every window and select the longest main Codex window for the headline. Never assume a 5-hour window or infer dollar spending. This is an internal endpoint and may change; a fresh browser sign-in and quota fetch are the live acceptance check. ChatGPT tokens are not guaranteed read-only, even though the app uses them only for refresh and usage.

The per-user process guard also covers `--check-codex`, preventing concurrent
refresh-token rotation between the tray and diagnostic processes. Loopback callback
header reads have a total deadline so an incomplete request cannot block sign-in.

## OpenCode Go (`opencode.rs`)

Browser device authorization follows the [upstream Console provider](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/plugin/provider/opencode.ts).
It uses direct HTTPS with client ID `opencode-cli` (the upstream protocol identifier,
not an executable dependency): POST `/console/auth/device/code` with
`supports_org_scope: true`, open `verification_uri_complete`, and poll
`/console/auth/device/token` with the device-code grant. Respect pending, slow-down,
declined and expired responses. Refresh through the same token endpoint.
Credentials belong to TokenKeeper and are saved under `opencode-go` using
DPAPI/Secret Service. No supplier CLI files or browser cookies are read.

Read `GET /console/api/go/status` with bearer authorization and `x-org-id`.
Use the organization from the authorization grant; if absent, discover through
`GET /console/api/orgs` and accept only a single workspace. Never arbitrarily
choose among multiple workspaces. Resolve organization scope after refresh,
including the single refresh-and-retry on 401. Save rotated tokens before any
subsequent request; failed persistence prevents further usage reads.

Parse `access.meters.fiveHour/week/month`, using `usedMicroCents / limitMicroCents`
for percentages. Numbers can be JSON numbers or decimal strings. Reset times
come from `resetsAt`; the monthly meter can fall back to `access.endsAt`.
Show every reported meter under Limits and the longest reported window in the
headline. Missing/invalid quota is an error. A 403 is an access/entitlement
error and preserves the session. Do not derive dollar spend, history or pace:
the response does not establish cycle start. Tokens and device codes never go
into response dumps or logs. `--check-opencode` shares the process guard.

Validation: the production device-code endpoint accepted the initial request
with HTTP 200 on 11 October 2026 (10-minute expiry, 5-second polling interval,
relative approval URL under `/console/device`). Device access-token permission
to read Console Go quota was subsequently confirmed by the user's successful
browser sign-in and usage test on Linux/KDE on the same date. Windows runtime
verification remains pending.
