# TokenKeeper

Windows and Linux/KDE tray app (Rust) showing Cursor, GitHub Copilot,
Claude Code, ChatGPT Codex and OpenCode Go usage. Start with `docs/design/README.md`:
- `docs/design/ui.md`: UI;
- `docs/design/providers.md`: endpoints and response quirks;
- `docs/design/deployment.md`: open cleanup items and the release checklist.

- Build: `taskkill /IM tokenkeeper.exe /F; cargo build --release` (the running exe is locked).
- Test: `cargo test --release`.
- Install: test and build, quit the running app, replace the installed executable, then restart. See `docs/design/deployment.md`.
- Keep local downloads, logs, previews and recovery backups under `.local/` (gitignored), not in `/tmp` or the system temp folder. Create the needed subdirectory before writing files.
- Preview UI changes: `target\release\tokenkeeper.exe --popup-sheet .local\previews\popups.png`, then view the PNG.
- Runtime data and debug dumps: `%APPDATA%\ai-usage` (`debug\log.txt`, `debug\*-error.txt`).
- Minimise user-installed runtime dependencies. Prefer crates bundled into the executable over extra system libraries or commands. The release profile is size-optimised.
- Claude requests must not use a browser-like User-Agent: Anthropic answers them with 429.

- Linux/KDE build: `cargo test --locked; cargo build --locked` (GTK3/Cairo runtime libraries required). Run `target/debug/tokenkeeper --demo` with isolated XDG paths for a non-credential UI preview, or `--demo-sheet .local/previews/panel.png` for offscreen Cairo rendering.
- All suppliers MUST use their web APIs directly. Never require, invoke, or import credentials from supplier-installed apps/CLIs. ChatGPT uses browser OAuth PKCE, direct HTTPS usage requests and app-owned secure credentials.
- First launch is idle: do not open provider sign-in or enable autostart automatically.
- Keep Linux and Windows builds/tests warning-free; verify with `RUSTFLAGS=-Dwarnings`.
