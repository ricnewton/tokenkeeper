# Linux popup toolkit evaluation

The production popup still uses GTK3/Cairo. Networking, hashing, Secret Service,
StatusNotifierItem/DBusMenu and browser launching have already moved to bundled
Rust clients (`81b71d7`), with native Windows/Linux CI and live desktop checks.
Replacing the popup is a separate change.

## Working comparisons

Credential-free experiments are in [experiments/ui](../../experiments/ui/README.md).
They use the same five-provider sample fixture, a 440×600 window, themes,
selection, tabs, charts and Escape. They do not read accounts or contact providers.
GTK3 uses the existing production demo (different sample amounts, same 42%
Cursor usage). The alternatives use the shared fixture. The fixed window size
keeps alternative screenshots comparable; responsive layout is not
being evaluated yet. Tabs change the chart heading, not a complete production
history implementation.

- **egui/eframe 0.36.2:** custom painter for the ring/chart; OpenGL through glow.
- **Iced 0.14:** message/update model, custom canvas ring and tiny-skia software
  rendering. No GPU backend enabled.
- **GPUI 0.2.2:** published release, separately evaluated rather than assuming
  Zed's Git-main popup API is available in the crate.

Release profiles use `opt-level = "s"`, no LTO, 16 codegen units in every experiment.
They differ from the production LTO build, so prototype sizes are not predictions
of the final app's size. Memory measurements are process RSS; they exclude GPU
allocations and vary by graphics driver, fonts and session.

Both egui and Iced passed warning-free locked release builds, rendered light/dark
previews on native Wayland and XWayland, and have no GTK linkage. This does not mean
zero system dependencies: window-system libraries are loaded dynamically; egui
also needs OpenGL, while Iced's selected renderer uses the CPU.

## Measured results on KDE, 11 October 2026

| Prototype | Release executable | Idle RSS | Idle CPU (one core) |
| --- | ---: | ---: | ---: |
| GTK3/Cairo production demo | 3.4 MiB | 34.6 MiB | 0.00% |
| egui/glow | 15.7 MiB | 93.8 MiB | 0.00% |
| Iced/tiny-skia | 11.8 MiB | 14.3 MiB | 0.00% |
| GPUI/Blade | 23.8 MiB | 83.0 MiB | 0.33% |

RSS was sampled after two seconds; CPU was sampled for the following three
seconds, without auto-close timers or interaction. GTK3 was the complete app in isolated demo mode, with the popup shown; the other
measurements are standalone panels. GTK3 dynamically links the installed toolkit,
while more code is bundled into the alternatives. These are short local samples,
not a portable benchmark or long-term power measurement. All three rendered on
native Wayland and XWayland. Light/dark images were inspected. XWayland Escape
passed for all three, and focus-loss dismissal passed for egui/Iced after guarding
against the initial unfocused creation event. GPUI has no focus-loss mode in this
experiment. Ordinary-window activation is not proof of activation from a tray.

GPUI required XCB/XKB development link names and a vendored xattr 0.2.3 Linux
compatibility patch; its 708-package resolved graph and Vulkan rendering introduce
more build/platform complexity. Runtime sonames were already installed; no system
packages were installed for the experiment. Cargo reports a future-compatibility
issue in proc-macro-error2 2.0.1. Details are in its experiment README.

**User preference:** GPUI's panel looks nicest; Iced is also good, but its initial
scrollbar/overflow needed correction. GPUI is the preferred visual style; the next work requested is improving Iced
and comparing it directly with GTK3 and GPUI.
Iced is the leading resource-use candidate. The production decision should weigh
that visual preference against GPUI's extra build/runtime requirements and verify
tray activation/placement before migration. The Iced comparison was subsequently
compacted to fit the entire fixed-size panel without overflow.

Stage 1 verification: 48 release tests passed, with two separately passing live
integration checks (browser portal and isolated wallet create/read/replace/delete).
Existing account usage, native menu actions/tooltips and installed binary linkage
were checked. Windows GNU compilation passed, and
[native Windows/Linux CI](https://github.com/ricnewton/tokenkeeper/actions/runs/38164201063)
passed. The verified 3,543,128-byte production binary is installed in
`~/.local/bin/tokenkeeper` and retains the GTK popup.

## KDE menu, tooltip and app popup

[Plasma's importer](https://github.com/KDE/plasma-workspace/blob/master/applets/systemtray/statusnotifieritemsource.cpp)
imports DBusMenu into a Qt menu. Our application controls labels, actions, icons,
enabled state, hierarchy and toggles. Plasma controls the native appearance.
Changing the application's GUI toolkit cannot change the toolkit Plasma uses.
Users can change their Plasma theme globally.

The [StatusNotifierItem protocol](https://specifications.freedesktop.org/status-notifier-item/latest/status-notifier-item.html)
also sends tooltip title, text and icon data. The host draws that hover tooltip;
there is no arbitrary interactive app widget or tray-pointer enter/leave stream.

Our click-to-open usage window belongs to the application and can use any toolkit.
A custom right-click window is possible by replacing native menu export with
ContextMenu handling, but it requires additional tray integration and gives up
Plasma's native menu behavior. It is not a library setting.

## Window integration remains a selection gate

Normal [winit Wayland toplevels](https://docs.rs/winit/latest/winit/window/struct.Window.html)
cannot set an absolute global position. XWayland offers the existing positioning
strategy; native Wayland needs a deliberate activation and placement design.
Undecorated prototype windows alone do not prove tray-popup parity.

Plasma sends `ProvideXdgActivationToken` before actions. ksni 0.3.6 does not expose
that extension; the previous GTK tray implementation also lacked it. Reliable
native Wayland activation needs separate work. Production migration must verify
tray clicks, focus dismissal/reopening, monitor clamping and fractional/mixed DPI.
These experiments have not verified Windows execution or mixed-monitor scaling.

[GPUI main](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)
and [published GPUI](https://docs.rs/gpui/0.2.2/gpui/) have different platform APIs.
Main's layer-shell flyouts can attach to a screen edge; its parent-anchored popups
require an application-owned parent surface/input serial, not Plasma's tray
surface. Do not infer tray anchoring or released Windows support from Git-main
examples. No Zed application source is copied into the prototypes.

All three framework licenses permit use in this MIT application: egui offers
MIT/Apache-2.0, Iced MIT and GPUI Apache-2.0. Retain relevant dependency notices.
Sources: [egui manifest](https://github.com/emilk/egui/blob/main/Cargo.toml),
[Iced manifest](https://github.com/iced-rs/iced/blob/0.14.0/Cargo.toml),
[GPUI manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml).
