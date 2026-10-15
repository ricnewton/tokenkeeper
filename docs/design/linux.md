# Linux desktop integration

The Linux popup uses Iced 0.14 with the tiny-skia software renderer. Windows
keeps its native Direct2D UI. Networking, hashing, Secret Service,
StatusNotifierItem/DBusMenu and browser launching use bundled Rust crates, so
GTK, Cairo, GIO, libsecret, libdbusmenu and OpenSSL are not linked.
Window-system libraries load dynamically.

## Why Iced

In October 2026, egui, Iced and GPUI prototypes each reproduced the borderless
three-card panel. They were measured on KDE against the earlier GTK3/Cairo
build:

| Toolkit | Release executable | Idle RSS |
| --- | ---: | ---: |
| GTK3/Cairo (previous production) | 3.4 MiB | 34.6 MiB |
| egui/glow | 15.7 MiB | 92.0 MiB |
| Iced/tiny-skia | 11.8 MiB | 14.2 MiB |
| GPUI/Blade | 23.8 MiB | 77.3 MiB |

Iced had the lowest memory use, needs no GPU, and has a simple build. GPUI
looked best, but it pulled in about 700 crates, needed Vulkan and a patched
dependency, and its published release lacks the popup APIs Zed uses. The
prototype sizes used a non-LTO profile. With full LTO, the production Linux
release is about 7.7 MiB and uses about 15 to 21 MiB RSS.

Canvas geometry is cached between polling ticks. It is invalidated when the
data, selection, theme, size or minute changes.

## KDE menu and tooltip

[Plasma imports DBusMenu](https://github.com/KDE/plasma-workspace/blob/master/applets/systemtray/statusnotifieritemsource.cpp)
into its own Qt menu. The app controls labels, actions, icons, enabled state,
hierarchy and toggles, but Plasma draws the menu. The app's toolkit cannot
change that.

The [StatusNotifierItem protocol](https://specifications.freedesktop.org/status-notifier-item/latest/status-notifier-item.html)
sends a tooltip title, text and icon, which the host draws. There is no
interactive hover widget and no pointer enter or leave events. A custom
right-click window would mean replacing the native menu with ContextMenu
handling.

## Window placement and activation

- **X11/XWayland** is preferred when available. It allows tray-anchored
  placement and timestamp-based activation (`linux_window.rs`). Dismissal hides
  and reuses the popup, which keeps the clipboard connection. Resize events
  reapply workarea clamping.
- **Native Wayland** (`TOKENKEEPER_BACKEND=wayland`) is compositor controlled.
  [winit toplevels](https://docs.rs/winit/latest/winit/window/struct.Window.html)
  cannot set a global position.
- Plasma sends `ProvideXdgActivationToken` before actions, but ksni 0.3 does
  not expose it. Reliable native Wayland activation needs separate work.

Still unverified: mixed-monitor and fractional DPI scaling.
