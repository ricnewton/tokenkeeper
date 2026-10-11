# Configuration and troubleshooting

## Light and dark appearance

**Windows** follows the system's app light/dark setting.

**Linux** defaults to dark. Light mode is supported explicitly; quit the running
copy, then launch it with:

```bash
TOKENKEEPER_THEME=light ~/.local/bin/tokenkeeper
```

Use `TOKENKEEPER_THEME=dark` for dark mode. This setting applies to the launched
process; set it in your desktop-session environment to use it with autostart.

## Credentials, data and privacy

TokenKeeper calls supplier web APIs directly. Credentials are stored using
Windows DPAPI or Linux Secret Service/KWallet. Signing out removes this app's
credentials; it does not sign out another application.

The app uses credentials for usage reads and token refresh. OAuth credentials
are **not guaranteed read-only**. Copilot requests `read:user`, Claude requests
`user:profile`, and ChatGPT requests `openid profile email offline_access`.
Cursor uses a session cookie.
OpenCode Go uses Console device authorization and an app-owned access/refresh
token; its permissions are controlled by OpenCode rather than a requested read-only scope.
Usage normally refreshes every five minutes; **Refresh now** requests an earlier update.

TokenKeeper preserves existing credentials, history and autostart settings across
upgrades. Windows stores data in the user's roaming application-data directory;
Linux uses the XDG data directory. Autostart uses the current user's Windows
`Run` registry key or a Linux desktop-session autostart entry. Existing installations
retain their original storage names; see [deployment details](../design/deployment.md) for storage names.

Diagnostics are in `debug/log.txt` under the data directory. To check your saved
ChatGPT login once, quit the tray app and run `tokenkeeper --check-codex`.
For OpenCode Go, choose **Accounts → OpenCode Go — Sign in…**, approve the
browser device code and select the workspace that owns your Go subscription.
Then check the Limits tab. To diagnose the saved session, quit the tray app and
run `tokenkeeper --check-opencode`. A denied or missing entitlement is shown
as an error, with no invented zero-usage snapshot.

To remove TokenKeeper, sign out of each account, disable autostart, quit, then
delete the executable and data directory. Remove any old executable alias too.

