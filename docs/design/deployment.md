# Deployment readiness and handover

## Current state (15 Oct 2026)

| Area | Status |
| --- | --- |
| Cursor | Done, used daily. Sign-in, refresh, summary, per-event breakdown and history all work |
| GitHub Copilot | Sign-in (device code card + clipboard) and usage verified on an Enterprise seat |
| Claude Code | Sign-in verified after the User-Agent fix. Usage verified only with zero spend on an Enterprise seat (see providers.md) |
| Multi-provider UI | Column glance and panel, active-provider switching by menu and mouse wheel, per-provider sign in/out |
| ChatGPT Codex | Browser sign-in, secure credential persistence and direct usage reads verified on Linux/KDE |
| OpenCode Go | User verified browser device sign-in and Console quota reads with a live subscription on Linux/KDE |
| Linux/KDE | Native StatusNotifier tray and Iced software-rendered panel (see linux.md); X11/XWayland tray activation and reopening checked; visual click-away review remains pending |
| Tests | Release tests cover providers, persistence, popup lifecycle, clipboard readiness and monitor geometry. Both platforms build and test warning-free in CI |
| Windows | Native tray and Direct2D panel in daily use; exe has version info and the ring icon |

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

1. **Confirm Claude spend units** once some credits have been used (open questions at the end of providers.md). Add a parser test built from the real response.

Done: Claude saves its usage response only when it is unrecognised; the extra-usage meter is hidden when spend is already the headline ring; diagnostics (`debug\log.txt` and response dumps) are off unless `TOKENKEEPER_DEBUG` is set. The log never records tokens or device codes; keep it free of secrets in future changes.

## Release checklist

- [x] **Version and exe metadata:** `build.rs` embeds a version resource, app icon and manifest with `winresource` (build-time only; needs the Windows SDK `rc.exe`). The icon is the tray ring rendered by `src/icon.rs` at 16 to 256 px, with a mid-grey track that reads on light and dark backgrounds. The resource sets the file and product version from `Cargo.toml`, and `FileDescription`/`ProductName` "TokenKeeper", which Windows shows as the app name (for example in Settings > Taskbar > Other system tray icons). Windows caches that name per exe path in `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache`; delete the stale entry if the old name persists. The manifest declares Windows 10/11 compatibility, per-monitor v2 DPI awareness (also set in code at startup) and `asInvoker`.
- [x] **License:** MIT, recorded in `LICENSE`, the README and Cargo package metadata.
- [x] **CI:** `.github/workflows/build.yml` runs native Windows/MSVC and Linux builds and tests, then uploads unsigned archives with the MIT license and installation instructions. `.github/workflows/release.yml` publishes a GitHub release from the `Cargo.toml` version. Toolchain: Rust 1.99.0, pinned in `rust-toolchain.toml`. No account credentials or signing secrets are needed. See the development guide for triggers and downloads.
- [x] **User README:** what the app does, how to run it, where data lives (`%APPDATA%\ai-usage`), how to remove it (quit, untick "Autostart", delete the folder), and a privacy note. Tokens are DPAPI-encrypted and stay on the machine, and Copilot and Claude use minimal scopes (`read:user`, `user:profile`); ChatGPT credentials must not be described as read-only.
- [ ] **Update path:** GitHub Releases now carry the downloads, but the app has no update check. The simplest option is a "new version available" check against the latest release.
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
