# UI design

Visual references: see [development and screenshots](../howto/development.md).
The `tokenkeeper-*.png` images in `mockups/` show current Linux panels with sample data;
older sheets and HTML mockups are historical.

## Tray icon (`icon.rs`)

- **Ring:** a progress ring showing the active provider's usage as a fraction of its allowance.
- **Pace tick:** a small mark on the ring at the fraction of the cycle that has elapsed. If the arc passes the tick, you're spending faster than the allowance lasts.
- **Colour:**
  - green below 80%;
  - amber from 80% to under 100%;
  - red at 100% or more (overage).
- **States:** an empty ring while loading or signed out.
- **Sizes:** rendered for the DPI-specific tray size (16, 20, 24 or 32 px), with variants for light and dark taskbars. Every 2 s a housekeeping timer re-checks `SystemUsesLightTheme` and the tray icon size, and re-renders the icon if either changed.
- **Tooltip:** the hover glance replaces it.

The icon always shows one provider, the **active** one:

- It defaults to Cursor.
- Change it from the menu ("Switch tray icon ▸") or by scrolling the mouse wheel over the icon.
- Tray icons don't get wheel messages, so the app installs a low-level mouse hook (`WH_MOUSE_LL`) only while the pointer hovers the icon. The hook swallows wheel events inside the icon's rectangle and posts `WM_WHEEL_SWITCH` to the UI thread. The popups also forward their own `WM_MOUSEWHEEL`.
- When a GitHub device code appears, the active provider switches to Copilot.

## Glance (hover card)

- **Behaviour:** a non-activating card that appears after hovering the icon for 350 ms. It hides 300 ms after the pointer leaves both the icon and the card.
- **One provider signed in:** the card is 300×176 DIP.
  - Left: the ring.
  - Right: the amount used, the allowance ("of $200.00 included" / "of the 7-day limit"), today's usage or the 5-hour session, and pace.
  - Header: the provider logo and name, a "Usage ↗" link and the plan badge.
  - Footer: the reset date and "Updated X".
- **Two or more providers signed in:** one 124-DIP column per signed-in provider.
  - Each column has its logo, short name, ring, amount, allowance, secondary line, pace and reset.
  - A dot marks the tray's active provider.
  - Each column header has a "↗" usage-page link. In the glance, clicking anywhere in a column opens that provider's usage page.
  - Under the columns, a combined bar shows the total dollars used against the total allowance ("All tools  $378.00 of $900.00 · 42%"), with one segment per provider in its brand colour. Providers that report percentages rather than dollars (Claude's rate-limit windows) or haven't loaded are left out, and the label then names the providers it adds up ("Cursor + Copilot"). Past the combined limit, a tick marks where the limit falls.
  - Signed-out providers are left out.
- **Device code:** while a GitHub device code is pending, the glance becomes a **pinned code card**. It shows the code in large type, says "Copied to clipboard · click to copy again", and stays up regardless of hover until the sign-in finishes or is cancelled.

## Panel (click flyout)

- **Behaviour:** a focused flyout like the Windows volume and network flyouts. It hides when it loses activation. A 400 ms guard stops the click that closed it from immediately reopening it.
- **Single provider:** the header and summary as in the glance, then tabs and a detail card:
  - Cursor and Copilot: **Today** (hourly bars), **This cycle** (daily bars against a budget-per-day line), **History** (past cycle totals against the limit).
  - Claude: **Limits** (one bar per rate-limit window, with reset times), then This cycle and History.
- **Several providers:** the column strip across the top, then the combined bar, with the tabs and detail card for the selected column below.
  - The selected column starts as the active provider; clicking another column selects it. Its "↗" opens that provider's usage page directly.
- **No breakdown yet:** the detail card says "Collecting data, check back later" (tracker-based providers) or "Breakdown not loaded yet" (Cursor).

The usage-page links go to cursor.com/dashboard?tab=usage, github.com/settings/copilot/features and claude.ai/settings/usage. Those pages carry the detail the app doesn't show, such as per-model usage.

## Menu (right-click)

The menu contains Accounts (provider-first sign in/out or cancel actions), Switch tray icon (active provider marked), Refresh now, the platform's autostart toggle, and Quit. Linux also offers Show usage panel. Usage summaries and provider dashboard links live in the panel, so their former tray submenus were removed.

## Theme and drawing

- **Drawing:** the popups use Direct2D and DirectWrite (Segoe UI), are per-monitor DPI aware, and have DWM rounded corners and shadow.
- **Theme:** colours follow the system light or dark app theme.
- **Brand colours:** Copilot `#8957E5`, Claude `#D97757`, Cursor neutral.
- **Placement:** popups sit next to the icon and are clamped to the monitor's work area, 12 DIP from the edge. If the content size changes while a popup is visible (for example, a provider signs in), it is re-laid-out at the same anchor.

## Linux/KDE panel and tray

`linux_app.rs` provides a dark GTK3/Cairo flyout with no title bar or taskbar entry. Left click toggles it; losing focus or pressing Escape hides it. A 400 ms guard prevents the click which dismissed it from reopening it. The panel omits signed-out columns, uses a compact single-provider layout, and scrolls when needed to show all limit meters. Activation coordinates select the monitor/workarea; placement is best-effort where a Wayland compositor controls window positioning. `TOKENKEEPER_THEME=light` explicitly selects light mode (`AI_USAGE_THEME` remains a compatibility alias).

`linux_tray.rs` implements KDE’s StatusNotifierItem protocol directly via GIO. It publishes the shared Windows ring renderer as network-order ARGB pixmaps at several tray sizes, emits icon-change signals, handles Activate and Scroll, and exports the existing GTK menu through libdbusmenu. Right-click account and provider-selection actions are grouped into Accounts and Switch tray icon; the selected provider is marked.

KDE supplies a hover summary from the live ToolTip property. The StatusNotifier protocol has no pointer-enter/leave events or tray rectangle, so the exact app-owned Windows hover card is not reproduced on Wayland. The hover data includes signed-in providers, current usage, plans and reset dates.

For X11/XWayland GTK windows, tray activation obtains a fresh X server timestamp and passes it to `gtk_window_present_with_time`. The native window selects property-change events before requesting the timestamp. This gives KWin a valid activation time so the panel receives focus immediately and can dismiss on the first click away. Other GTK display backends retain normal presentation behavior.

### Account menu decisions

Task: inspect account state and connect, disconnect or cancel without opening another screen. Provider names lead each row for stable scanning; connected accounts show the explicit Sign out action. Signed-out accounts retain an enabled Sign in action, while in-progress authentication offers Cancel sign-in. Only the initial credential-loading state is disabled. This combines state and the next action in four flat rows, without another level of submenus or unreliable native font-weight styling. The Sign out action indicates connection, not quota or fetch success; a failed usage request keeps it available. Linux selection and enabled autostart indicators use text suffixes so provider names retain their alignment. Windows uses native menu check columns for tray selection and Autostart. Linux labels Autostart explicitly as On or Off.
