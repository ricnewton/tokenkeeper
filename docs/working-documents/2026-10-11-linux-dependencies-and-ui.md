# Linux dependency migration and UI evaluation

## Completion contract

Replace Linux SHA-256, HTTP, wallet access, tray protocol/menu export and browser launching with compiled Rust dependencies. Retain GTK3/Cairo popup rendering for the first commit. Preserve existing Secret Service attributes and account data, supplier API behavior, native KDE menus, idle first launch and Windows behavior.

Proof: warning-free locked release tests/build; Windows compile check and native CI; HTTP transport regression tests; isolated live wallet create/read/delete and existing account usage checks without exposing tokens; desktop tray/menu protocol inspection; executable linkage inspection; preview rendering.

Commit and push this stage before changing the popup toolkit. Then compare egui/eframe, Zed GPUI and Iced against tray popup activation/placement, focus dismissal, rendering/scaling, Windows/Linux support, runtime dependencies and licensing. Build a bounded credential-free prototype of the best-supported candidate; retain production GTK until the experiment has demonstrated suitable behavior.

Local artifacts belong in gitignored `.local/`. Never import supplier CLI credentials. Never log credential contents. Stop for actual account data loss or incompatible credential migration; preserve established storage attributes.

## Decisions and evidence

- The installation burden, rather than the count of Cargo dependencies, is the priority.
- KDE renders exported D-Bus menus and tooltips. The app controls the menu model and owns its separate usage window.
- Wayland compositor policy limits arbitrary positioning of independent application windows; evaluate this before selecting a new GUI toolkit.

## Stage 1 verification

- Locked warning-free release suite: 48 passed; two desktop integration tests excluded from ordinary CI.
- Both desktop integration tests separately passed: browser OpenURI portal and isolated KWallet create/read/replace/delete.
- Existing OpenCode Go and Codex credentials loaded with the original attributes; live usage requests succeeded through rustls. The tray also loaded existing Cursor credentials and fetched usage.
- Native menu layout and tooltip inspected through D-Bus; account labels, five-provider switching and autostart state preserved.
- Windows GNU check passed. Native Windows/Linux CI follows the commit.
- Executable links to GTK/Cairo, with GIO indirectly through GTK; no libcrypto/OpenSSL, libsecret or libdbusmenu linkage. Binary size approximately 3.5 MB before the final SOCKS additions.
- Independent review identified missing SOCKS support/proxy precedence and missing default-wallet creation. Corrected; regression checks added for proxy precedence/bypass/SOCKS. Missing default alias now creates a persistent default collection only on NoResult.

## UI comparison

Initial recommendation: prototype egui/eframe and compare Iced with software rendering. GPUI is promising for Wayland layer-shell flyouts but current Git main and published APIs differ; its wider renderer/text/platform dependency graph needs justification for this small app.

- egui/eframe: MIT or Apache-2.0; custom painter matches the current rings/charts. Choose glow instead of the default GPU stack for a smaller baseline. https://github.com/emilk/egui/blob/main/crates/eframe/Cargo.toml
- Iced: MIT; structured messages/widgets and a tiny-skia software-rendering option. https://github.com/iced-rs/iced/blob/0.14.0/Cargo.toml
- GPUI: Apache-2.0; published 0.2.2 lacks newer main-branch platform/popup APIs. https://docs.rs/gpui/0.2.2/gpui/ and https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md
- Plasma imports the application's D-Bus menu and renders it itself: https://github.com/KDE/plasma-workspace/blob/master/applets/systemtray/statusnotifieritemsource.cpp
- Normal Wayland toplevels cannot set global position; activation tokens and compositor policy determine focus. GPUI's parent-anchored popup needs an app-owned parent surface, which the Plasma tray does not supply. Layer-shell offers screen-edge placement rather than actual tray-icon anchoring.
- ksni 0.3.6 lacks Plasma's ProvideXdgActivationToken extension, as did the previous implementation. Evaluate that separately before claiming native Wayland flyout parity.
