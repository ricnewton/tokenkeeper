# Deployment readiness and handover

## Current state (11 Oct 2026)

| Area | Status |
| --- | --- |
| Cursor | Done, used daily. Sign-in, refresh, summary, per-event breakdown and history all work |
| GitHub Copilot | Sign-in (device code card + clipboard) and usage verified on an Enterprise seat |
| Claude Code | Sign-in verified after the User-Agent fix. Usage verified only with zero spend on an Enterprise seat (see providers.md) |
| Multi-provider UI | Column glance and panel, active-provider switching by menu and mouse wheel, per-provider sign in/out |
| ChatGPT Codex | Browser sign-in, secure credential persistence and direct usage reads verified on Linux/KDE |
| OpenCode Go | User verified browser device sign-in and Console quota reads with a live subscription on Linux/KDE |
| Linux/KDE | Native StatusNotifier tray, GTK/Cairo panel, click-away dismissal, light-mode option and autostart verified |
| Tests | 43 tests covering parsing, callbacks, PKCE, refresh rotation and persistence ordering, workspace scope, process exclusion and provider switching; warning-free Linux release and Windows cross-check |
| Windows follow-up | Runtime testing of the port remains pending; compilation has passed |

## Installing and updating

Run `cargo test --release --locked` and `cargo build --release --locked`.
If tests or the build fail, keep the existing installation running.

On Windows, quit TokenKeeper before building or replacing a running executable.
Copy `target\release\tokenkeeper.exe` to the installation directory you chose,
then start it. Builds are unsigned; installation requires no signing tool or
certificate.

On Linux, quit TokenKeeper, then install and start the verified build:

```bash
install -m 755 target/release/tokenkeeper ~/.local/bin/tokenkeeper
~/.local/bin/tokenkeeper
```

Keep the previous executable if you want a rollback. Replacing the executable
does not replace stored accounts, usage history or preferences.

**Autostart follows the exe.** On Windows, the `ai-usage` value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` stores the executable's
full path. On Linux, autostart uses a desktop entry. Launching a moved or updated
copy re-points an existing enabled entry at that executable. If autostart is off,
it stays off. First launch is idle until the user chooses to connect an account.

## Cleanup before the first release

The product and executable are now named TokenKeeper (`tokenkeeper.exe` / `tokenkeeper`). Existing storage, credential, autostart and single-instance identifiers retain `ai-usage` for compatibility. On Linux the installed release is `~/.local/bin/tokenkeeper`; the old command can remain a compatibility symlink. Existing enabled autostart entries follow the renamed executable on launch.

1. **Stop the unconditional Claude dump.** `claude.rs` writes every usage response to `debug\claude-usage.json` (`save_debug("claude-usage.json", …)` in `fetch`). It was added to learn the Enterprise shape. Either remove it, or keep it only for unexpected responses as the other providers do.
2. **Confirm Claude spend units** once some credits have been used (open questions at the end of providers.md). Add a parser test built from the real response.
3. **Hide the duplicate Claude meter.** When the headline ring is already the extra-usage spend, don't also list "Extra usage this month" on the Limits tab.
4. **Refresh `mockups/`.** `icon-sheet.png` and `popup-sheet.png` predate the multi-provider work. Regenerate them with `--icon-sheet` and `--popup-sheet`. The sheet's PNG encoder stores data uncompressed, so recompress the files before committing (previously done by re-saving through System.Drawing). `mockups/popup.html` is an early HTML mockup; delete it or label it as historical.
5. **Debug log.** `debug\log.txt` always records sign-in, sign-out and error events (`worker.rs`); it never records tokens or device codes, and it is capped at 256 KB. Decide whether to keep it on in release builds. It has been useful for remote diagnosis. Keep it free of secrets in future changes.

## Release checklist

- [ ] **Version and exe metadata:** there is no `build.rs`, so the exe has no icon resource, version info or manifest. Add a `build.rs` (for example with `winresource`, or a hand-written `.rc` built with `embed-resource`) that sets:
  - the file and product version from `Cargo.toml`;
  - the company name and description;
  - an app icon (the ring);
  - optionally, a manifest. DPI awareness is already set in code (`SetProcessDpiAwarenessContext` per-monitor v2 in `main.rs`), but a manifest is the recommended way.
- [x] **License:** MIT, recorded in `LICENSE`, the README and Cargo package metadata.
- [ ] **CI:** GitHub Actions jobs on Windows and Linux running tests and release builds, then uploading unsigned binaries as artifacts. Linux runners need the desktop runtime libraries from the setup guide. No account credentials or signing secrets are needed for these build jobs. Pin the toolchain with a `rust-toolchain.toml` (developed on Rust 1.99).
- [x] **User README:** what the app does, how to run it, where data lives (`%APPDATA%\ai-usage`), how to remove it (quit, untick "Autostart", delete the folder), and a privacy note. Tokens are DPAPI-encrypted and stay on the machine, and Copilot and Claude use minimal scopes (`read:user`, `user:profile`); ChatGPT credentials must not be described as read-only.
- [ ] **Update path:** there's none yet. The simplest option is a "new version available" check against GitHub Releases, or just documenting how to replace the exe in place.
  - For now, replace the installed executable in place. Autostart re-points itself at whichever copy runs (see "Installing and updating").

## Risks to keep in mind

- **Undocumented APIs.** Any provider can change shape or start rejecting third-party clients. Mitigations:
  - defensive parsing;
  - debug dumps of unexpected responses;
  - visible "last update failed: …" states that keep showing stale data.
- **Claude client restrictions.** A browser-like User-Agent triggers 429s. If supplier policies change, adapt the direct web sign-in or show an actionable failure; never import another app's credentials.
- **Copilot client ID.** `copilot_internal` only accepts the Copilot editor app's tokens (`Iv1.b507a08c87ecfe98`).
- **Polling load.** Each provider is polled every 5 minutes. Cursor's event sync is incremental (the last 2 hours plus new events). Keep it that way if more history is added.

## Possible later features

- Tune the amber and red shades.
- Draw a small provider badge on the tray icon, so you can tell which provider the ring is showing.
- Pause polling while the session is locked or the machine is asleep (`WTSRegisterSessionNotification`), and refresh on unlock.
- Per-model breakdown for Cursor (the usage events include the model). The "Usage ↗" link covers this for now.

## Development gotchas

- **File locking:** while the app runs, `cargo build` fails with "Access is denied" because Windows locks the exe. Run `taskkill /IM tokenkeeper.exe /F` first.
- **Editor tool writes:** in this repo, edits to an open `.rs` file sometimes failed with `ftruncate` / "user-mapped section open", probably because rust-analyzer had the file memory-mapped. The workaround was to move the file aside and write a fresh copy.
- **curl on the corporate network:** use `curl.exe --ssl-no-revoke`. The revocation check fails behind TLS inspection; WinHTTP is unaffected.
- **Clipboard:** `OpenClipboard` must be given an owner HWND. With `NULL`, `SetClipboardData` fails after `EmptyClipboard`.
