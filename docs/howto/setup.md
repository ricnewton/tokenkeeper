# Installation and autostart

### Windows

Run `tokenkeeper.exe`. No installer or administrator rights are required.
Enable **Autostart** in the tray menu to start it when you sign in.

To build from source with Rust:

```powershell
cargo test --release --locked
cargo build --release --locked
.\target\release\tokenkeeper.exe
```

Quit the running Windows copy before rebuilding; Windows locks its executable.
After building, quit the running copy, replace the installed executable with
`target\release\tokenkeeper.exe`, then launch it again. No signing tool is required.

### Linux / KDE

The Linux popup uses Iced with a bundled software renderer. It does not require
GTK3, Cairo, OpenGL or Vulkan. A normal X11/XWayland or Wayland desktop supplies
the window-system libraries and fonts. Minimal Ubuntu installations may need:

```bash
sudo apt install ca-certificates libx11-6 libxkbcommon0 libwayland-client0 fonts-dejavu-core
```

Package names may differ on other distributions. A running Secret Service,
such as KWallet's Secret Service integration, is required to save credentials.
The desktop must provide StatusNotifier tray support and an XDG OpenURI portal
(KDE normally supplies these). The app talks to these services directly through
Rust D-Bus clients; libsecret, libdbusmenu, curl, OpenSSL and xdg-utils are not
app runtime requirements. GTK3, Cairo and GIO are also no longer required. Existing saved accounts remain compatible.

With Rust installed:

```bash
cargo test --release --locked
cargo build --release --locked
mkdir -p ~/.local/bin
install -m 755 target/release/tokenkeeper ~/.local/bin/tokenkeeper
~/.local/bin/tokenkeeper
```

Click **Autostart - Off** in the tray menu to enable desktop-login autostart;
the label changes to **Autostart - On**.
An existing enabled entry follows the executable when you move it.
KDE controls native hover tooltips. TokenKeeper prefers X11/XWayland when
`DISPLAY` is available, allowing tray-relative positioning and timestamp-based
activation. Set `TOKENKEEPER_BACKEND=wayland` to select native Wayland, or
`TOKENKEEPER_BACKEND=x11` to explicitly select X11. Native Wayland placement and
activation remain compositor controlled. `TOKENKEEPER_THEME=light` selects light
mode; dark is the default.

