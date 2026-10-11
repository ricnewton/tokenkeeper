# GPUI prototype

This credential-free prototype pins the published `gpui` 0.2.2 crate. It shares the app-owned fixture in `../shared/fixture.rs` with the egui and Iced prototypes. It has no provider, wallet, tray, autostart, or network code.

```sh
CARGO_TARGET_DIR="$PWD/.local/ui-build" cargo run --manifest-path experiments/ui/gpui/Cargo.toml
CARGO_TARGET_DIR="$PWD/.local/ui-build" cargo run --manifest-path experiments/ui/gpui/Cargo.toml -- --auto-close 3
```

The window is 440×600. `--light` selects the light theme and `--auto-close SECONDS` bounds a smoke run. Escape closes the application. GPUI 0.2.2 does not expose a portable window screenshot API; its Wayland platform source also says screen capture is not implemented, so this prototype does not implement `--screenshot`.

## Initial hypotheses and limitations

- GPUI's retained element model and native window controls may suit an app-owned panel, but this comparison uses the published API, not newer Git-main popup APIs.
- The 0.2.2 default feature set enables both X11 and Wayland. It uses Blade GPU rendering and includes native font and clipboard integrations; Linux runtime/build requirements may exceed the other prototypes and need desktop verification.
- GPUI 0.2.2 pulls `xattr` 0.2.3 through its HTTP archive helper; that release references `libc::ENOATTR` on Linux, where libc exposes the equivalent `ENODATA`. A small local patch applies that platform alias for this standalone prototype.
- The ring uses a custom GPUI canvas stroke for the sample percentage; the bar and chart use retained elements. This is a layout and interaction comparison, not a tray integration claim.
- Ordinary Wayland toplevel placement remains compositor controlled. The app has no Plasma tray surface to parent a GPUI popup to.

## Build requirements and evidence

On Debian/Ubuntu the published Linux backend needs the development link names
provided by `libxcb1-dev`, `libxkbcommon-dev` and `libxkbcommon-x11-dev`, plus the
usual Rust/C linker toolchain. The runtime needs the corresponding XCB/XKB
libraries, fonts/font configuration and a working Vulkan driver. It does not use GTK.

The current machine has the runtime libraries but lacks those development links.
The experiment was built without installing packages: local symlinks under
`.local/ui-build/native-libs` point to the installed library sonames, and
`LIBRARY_PATH="$PWD/.local/ui-build/native-libs" RUSTFLAGS=-Dwarnings` supplies the
link search path. This directory is ignored and is not distributed.

Locked release builds and native Wayland/XWayland runs passed. XWayland Escape
passed. App-window previews were captured with ffmpeg X11 window capture, without
capturing the rest of the desktop. Native Wayland image capture, focus-loss
popup behavior, tray positioning and Windows execution remain unverified.
Cargo reports a future-compatibility issue in transitive `proc-macro-error2` 2.0.1,
although current compiler checks succeed with `-Dwarnings`.
The vendored xattr compatibility patch retains its MIT/Apache license files.
