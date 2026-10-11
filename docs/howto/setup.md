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

The Linux build uses GTK3, Cairo, GIO, libdbusmenu and Secret Service. For Ubuntu,
install the runtime libraries:

```bash
sudo apt install libgtk-3-0t64 libdbusmenu-gtk3-4 libdbusmenu-glib4 libcairo2 libsecret-1-0 libssl3t64 curl xdg-utils
```

Package names may differ on other distributions. A running Secret Service,
such as KWallet's Secret Service integration, is required to save credentials.

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
KDE controls native hover tooltips; exact flyout positioning can depend on the compositor.

