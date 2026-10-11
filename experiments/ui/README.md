# Linux UI comparison prototypes

These are isolated, credential-free UI experiments. They do not use the production crate, tray, wallet, provider APIs, network, or autostart. The five-provider fixture is shared by path from `shared/fixture.rs`.

Build and inspect either app from the repository root:

```sh
export CARGO_TARGET_DIR="$PWD/.local/ui-build"
cargo run --release --locked --manifest-path experiments/ui/egui/Cargo.toml
cargo run --release --locked --manifest-path experiments/ui/iced/Cargo.toml
```

egui and Iced open as ordinary 440×600 windows. `--auto-close SECONDS` supports bounded smoke runs; `--screenshot PATH` saves a PNG after the initial frame and then closes; `--light` starts in the light theme. `--popup` removes window decorations and closes on focus loss. Escape closes either window. Switch providers, tabs, and theme; inspect the chart. Iced was compacted so the full panel fits without a scrollbar. Backend selection follows the session environment by default; `--x11` clears Wayland display variables and `--wayland` clears `DISPLAY` for a forced backend check.

## Initial hypotheses and limits

- egui/eframe should make custom usage rings and charts straightforward and keep the rendering path compact with glow.
- Iced should make the view and message flow easier to structure; tiny-skia exercises the software-rendering option.
- Both are expected to work as normal X11 and Wayland toplevels. Neither can globally position a normal Wayland window; compositor policy governs placement and focus.
- KDE renders the exported tray menu itself. These prototypes only evaluate the app-owned usage window and do not claim tray activation, menu parity, or tray-anchored popup behavior.
- `--popup` is an app-owned undecorated window, not a tray-anchored popup. Wayland compositor policy still governs placement and activation.
- The PNG capture uses each toolkit's window framebuffer; the capture path is local and created as needed.

## GPUI comparison

The third prototype is [GPUI 0.2.2](gpui/README.md). It uses the same fixture,
provider/tab/theme controls, custom ring and chart, and a fixed 440×600 window.
It supports `--light` and `--auto-close`; Escape closes it. It currently has no
focus-loss popup mode or portable PNG capture. See its README for the additional
Linux build libraries and the compatibility patch needed by the published crate.

Local release binaries are `.local/ui-build/release/ui-egui-prototype`,
`ui-iced-prototype` and `ui-gpui-prototype`. See the
[measured evaluation](../../docs/design/ui-toolkit-evaluation.md).
