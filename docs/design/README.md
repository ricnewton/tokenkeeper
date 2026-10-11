# TokenKeeper: design overview

A Windows and Linux tray app that shows how much of your AI-tool allowance you've
used: Cursor, GitHub Copilot, Claude Code, ChatGPT Codex and OpenCode Go. Windows ships as
a size-optimised exe; Linux uses GTK3/Cairo for the popup, with Rust clients for desktop services.

| Document | Covers |
| --- | --- |
| [ui.md](ui.md) | Tray icon, hover glance, click panel, menu, interactions |
| [providers.md](providers.md) | Sign-in and usage endpoints per provider, response quirks |
| [deployment.md](deployment.md) | Current state, known gaps, release checklist |

For setup and development commands, see the [how-to guides](../howto/setup.md)
and [screenshot workflow](../howto/development.md).

## Goals and constraints

- **Glanceable:** the tray icon alone tells you roughly where you are against your allowance and against the pace you'd need to last the cycle.
- **Shareable:** one exe that a colleague can drop anywhere and run. No admin rights (autostart uses the per-user Run key) and nothing written outside `%APPDATA%\ai-usage`.
- **Corporate-network safe:** all HTTPS goes through WinHTTP, so it honours the system proxy and the Windows certificate store (TLS inspection).
- **Secure credentials:** Windows uses DPAPI; Linux uses Secret Service/KWallet.
- **Direct web APIs:** never invoke supplier apps/CLIs or import their credential files. ChatGPT credentials are not guaranteed read-only.
- **Explicit setup:** first launch stays idle; sign-in and autostart are user choices.

## Architecture

```
            UI thread (main.rs)                         worker threads (worker.rs)
 ┌──────────────────────────────────────┐       ┌──────────────────────────────────┐
 │ tray-icon + Win32 message loop       │ Cmd   │ Worker<Cursor>   ─┐              │
 │ App { slots[3], active, glance,      │──────▶│ Worker<Copilot>   ├─ Provider    │
 │       panel, hover state, hook }     │       │ Worker<Claude>   ─┘  trait       │
 │                                      │◀──────│                                  │
 │ popup.rs (Direct2D/DirectWrite)      │ Event │ http.rs (WinHTTP, blocking)      │
 │ icon.rs (software ring renderer)     │ + PostThreadMessage(WM_WORKER_EVENT)    │
 └──────────────────────────────────────┘       └──────────────────────────────────┘
                                                  store.rs: %APPDATA%\ai-usage
```

- **The UI thread never blocks on the network.** Each provider has one worker thread that owns its session (tokens, caches), signs in, refreshes tokens and polls.
- **Commands** (`Cmd::{RefreshNow, SignIn, SignOut}`) go to a worker over its own channel.
- **Events** (`Event::{SignedOut(reason), SigningIn(code), Usage(Snapshot), Error(msg)}`) come back on one shared channel tagged with `ProviderId`. The worker then posts `WM_WORKER_EVENT` to wake the UI thread.
- **Polling:** every 5 minutes by default (`Provider::poll_every`). Manual refreshes are at least 15 s apart. After an error the retry wait starts at 30 s and doubles up to the poll interval.
- **Single instance:** a named mutex, `Local\ai-usage-tray-single-instance`.

### The `Provider` trait (`provider.rs`)

```rust
trait Provider: Send {
    fn id(&self) -> ProviderId;
    fn load(&mut self) -> bool;                          // stored tokens present?
    fn begin_login(&mut self) -> Result<Box<dyn Login>, String>;
    fn accept_login(&mut self, creds: Value) -> Result<(), String>;
    fn sign_out(&mut self);
    fn fetch(&mut self, send: &mut dyn FnMut(Snapshot)) -> Result<(), FetchError>;
    fn poll_every(&self) -> Duration;                    // default 5 min
}
trait Login { fn url(&self) -> &str; fn user_code(&self) -> Option<&str>; fn poll_interval(&self) -> Duration; fn poll(&mut self) -> PollResult; }
```

- **Sign-in:** the worker opens `Login::url()` in the default browser. It then calls `poll()` at `poll_interval()` until the result is `Done(creds)`, `Failed(msg)` or a cancel. If the login has a `user_code`, that's a device-flow code, and the UI shows it on a pinned card and copies it to the clipboard.
- **Fetching:** `fetch` may call `send` more than once. Cursor does this: it sends the quick summary first, then the per-event breakdown.
- **Signed out:** `FetchError::SignedOut` means the server rejected the stored session. The worker deletes the tokens and shows "sign in again".

Adding a provider means adding a `ProviderId` variant, implementing `Provider`, and registering it in `main_windows.rs` and `linux_app.rs`. The menu and popups are data-driven.

### Data model (`model.rs`)

A provider-neutral `Snapshot` contains:

- `unit`: Cents (Copilot credits count as cents: 100 credits = $1), or Basis (hundredths of a percent, for Claude's percentage windows).
- `used` and `limit`.
- Cycle start and end.
- `plan`.
- `meters`: extra bars, such as Claude's 5-hour and weekly windows.
- `details`: which panel tabs to show (`Today`, `Cycle`, `History`, `Limits`).
- `daily`, `hourly`, `today` and `history` breakdowns.
- `empty_note`.

`View` is the UI state for each provider: `Loading`, `SignedOut(reason)`, `SigningIn{code}`, `Failed(msg)`, or `Ready{snap, error}`. In `Ready`, `error` is the most recent failure, shown over stale data.

### Usage breakdowns

- **Cursor** exposes per-request usage events, so its hourly, daily and cycle breakdowns are exact (`cursor.rs`, `Breakdown`). Every poll re-fetches the last 2 hours, because an event's cost can be filled in after the event appears. Totals for past cycles are cached in `cursor-history.json`.
- **Copilot and Claude** only report a running total. `tracker.rs` records a reading at each poll and assigns the growth since the last reading to the hour it was first seen. As a result:
  - Usage while the PC was off lands in the next hour the app observes.
  - Whatever was already used at the first reading of a cycle is split evenly across the earlier days. Today and the hourly view show only observed usage. Until there's a reading, the panel says "Collecting data, check back later".
  - Readings within 2 hours of a cycle boundary count as the same cycle.
  - Up to 5 past cycles are kept, in `copilot-history.json` and `claude-history.json`.

### Storage (`store.rs`), all under `%APPDATA%\ai-usage`

| File | Contents |
| --- | --- |
| `{cursor,copilot,claude}-tokens.bin` | DPAPI-encrypted JSON tokens (`crypto.rs`) |
| `{cursor,copilot,claude}-history.json` | Past-cycle totals, plus the tracker's hourly readings |
| `debug\log.txt` | Timestamped worker events. Starts afresh past 256 KB |
| `debug\*-error.txt`, `debug\claude-usage.json` | Last unexpected response body for each failure point (see deployment.md) |

## Source map

| File | Role |
| --- | --- |
| `main.rs` | Platform entry point and `--check-codex` read-only usage command |
| `main_windows.rs`, `popup.rs`, `clipboard.rs`, `sheet.rs` | Windows tray, Direct2D panel, clipboard and previews |
| `linux_tray.rs` | Rust ksni StatusNotifierItem and native menus, ARGB icons, activation, wheel and hover summary |
| `linux_app.rs` | GTK3/Cairo flyout, direct KDE tray integration, menu and demo rendering |
| `worker.rs`, `provider.rs`, `model.rs` | Shared polling/sign-in worker, provider traits and UI snapshots |
| `cursor.rs`, `copilot.rs`, `claude.rs`, `codex.rs`, `opencode.rs` | Shared provider implementations; ChatGPT and OpenCode use browser authorization and supplier HTTPS endpoints |
| `tracker.rs`, `mock.rs`, `icon.rs` | Usage histories, demo snapshots and ring renderer |
| `http_windows.rs`, `crypto_windows.rs`, `store_windows.rs`, `time_windows.rs`, `autostart_windows.rs` | Windows WinHTTP, CNG/DPAPI, AppData, calendar and Run key |
| `http_linux.rs`, `crypto_linux.rs`, `store_linux.rs`, `time_linux.rs`, `autostart_linux.rs` | Linux ureq/rustls, RustCrypto/OS RNG, Rust Secret Service and XDG paths |

The older architecture examples above describe the Windows host. On Linux, the GTK loop polls the same worker-event channel rather than receiving `WM_WORKER_EVENT`. See [ui.md](ui.md) for the deliberate Linux interaction differences and [providers.md](providers.md) for the Codex account contract.

## Building and dev tools

See [development and screenshots](../howto/development.md) for build checks,
preview commands and Windows rebuild constraints.
