# Linux Iced migration

Objective: replace Linux GTK3/Cairo popup and clipboard use with bundled Iced,
preserving custom cards, real provider workers, native KDE menu, sign-in codes,
tabs/history, first-launch idle behavior, and click-away/Escape dismissal.
Windows retains its native implementation. Keep existing full LTO/size profile.

Proof: warning-free locked release tests/build, Windows cross-check, Linux
render previews, linked-library inspection, daemon/tray activation and reopening,
real account reads after deployment. Inspect new binary size and process memory.

Constraints: no supplier CLI or credential imports; no autostart/sign-in changes;
no standard provider buttons/dropdowns; generated artifacts under `.local/`.
Native Wayland compositor controls positioning; don't claim tray anchoring without
proof. Stop for destructive work, conflicting edits, data risk or repeated failure.

Execution: parent integrates daemon/lifecycle; bounded worker implements renderer
in a separate module; independent reviewer inspects final behavior and evidence.

Completed: Linux release built and installed; provider usage verified; 53 tests
passed, 2 optional desktop checks skipped; Windows cross-check passed; dark/light
framebuffers captured on both backends. Independent review caught resize placement
and clipboard worker lifecycle issues, both fixed. Repeated activation counters
remain stable. Final resource measurements are in ui-toolkit-evaluation.md.

Pending review: user visual interaction and real code paste, mixed-monitor DPI,
and native Windows execution. Native CI will run on push.
