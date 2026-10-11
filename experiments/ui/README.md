# Linux UI comparison prototypes

These isolated, credential-free experiments reproduce the supplied GTK3 flyout
in egui, Iced and GPUI. Each opens a borderless 440×606 window with three custom
account cards: Cursor 6%, Codex 33% (selected initially), and OpenCode 0%. The
selection marker, usage rings, plan badge, Limits panel, meter and footer follow
the reference. There are no standard provider buttons, dropdowns or scrollbars.

The data is fixed demonstration data. Clicking a card selects an account;
scrolling cycles accounts and Escape closes the window. Only the Limits view is
mocked; full provider-specific history, authentication, refresh and tray behavior
are outside this experiment. “Usage >” is decorative. No prototype reads
credentials or uses the production crate, wallet, APIs, network or autostart.

Build from the repository root:

```sh
export CARGO_TARGET_DIR="$PWD/.local/ui-build"
RUSTFLAGS=-Dwarnings cargo run --release --locked --manifest-path experiments/ui/egui/Cargo.toml
RUSTFLAGS=-Dwarnings cargo run --release --locked --manifest-path experiments/ui/iced/Cargo.toml
```

For GPUI, see [its build requirements](gpui/README.md). Local release binaries are
`.local/ui-build/release/ui-egui-prototype`, `ui-iced-prototype` and
`ui-gpui-prototype`.

All three support `--light` and `--auto-close SECONDS`. egui and Iced also support
`--screenshot PATH`, which captures the initial framebuffer to a PNG and closes,
and `--popup`, which adds focus-loss dismissal. Borderless appearance is already
the default. Leaving focus-loss dismissal off makes side-by-side review possible.
Their `--x11` and `--wayland` flags force the backend by clearing the other display
variables. GPUI follows the session environment and has no portable PNG capture
or focus-loss dismissal in this experiment.

Native Wayland and XWayland startup and dark/light rendering passed for all
three. egui/Iced card switching and wheel cycling passed desktop checks. GPUI's
card hit handling was subsequently revised; interactive confirmation of that
revision remains pending. Escape passed on XWayland for all three.

KDE owns its tray menu and tooltip. These prototypes evaluate the app-owned usage
window. An undecorated toplevel does not establish tray anchoring, activation,
focus behavior, monitor clamping or mixed DPI parity. Those remain migration
gates, alongside Windows execution. Production continues to use GTK3.

See the [measured evaluation](../../docs/design/ui-toolkit-evaluation.md).
