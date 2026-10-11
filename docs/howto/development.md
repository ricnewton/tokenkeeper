# Development and screenshots

Linux screenshots can be rendered without signing in or opening a desktop window:

```bash
target/release/tokenkeeper --demo-sheet /tmp/tokenkeeper-dark.png --demo-provider codex --demo-tab 0 --demo-single
TOKENKEEPER_THEME=light target/release/tokenkeeper --demo-sheet /tmp/tokenkeeper-light.png --demo-provider codex --demo-tab 0 --demo-single
target/release/tokenkeeper --demo-sheet /tmp/tokenkeeper-providers.png --demo-provider cursor --demo-tab 1
```

`--demo` opens a sample panel without accessing credentials or changing autostart.
Windows provides visual previews in both themes:

```powershell
.\target\release\tokenkeeper.exe --icon-sheet $env:TEMP\icons.png
.\target\release\tokenkeeper.exe --popup-sheet $env:TEMP\popups.png
```

Inspect the generated PNG before relaunching to check layout changes.

## Build checks

Keep both platforms warning-free. On Linux:

```bash
RUSTFLAGS=-Dwarnings cargo test --release --locked
RUSTFLAGS=-Dwarnings cargo build --release --locked
```

With the Windows GNU Rust target and its toolchain installed, also check the
Windows source:

```bash
RUSTFLAGS=-Dwarnings cargo check --locked --target x86_64-pc-windows-gnu
```

A cross-check verifies compilation; test tray interactions on Windows itself.
Quit the running Windows copy before rebuilding because Windows locks the executable.
See [deployment](../design/deployment.md) for installation and update steps.
