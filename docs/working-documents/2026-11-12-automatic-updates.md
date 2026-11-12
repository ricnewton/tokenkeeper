# Automatic updates on Windows and Linux

Research date: 12 November 2026. The research below preceded implementation; see the implementation record at the end.

## Recommendation

Keep the current portable executable distribution. Add background release detection,
an in-app update message and an **Update and restart** action. Evaluate `self_update`
first for GitHub discovery, archive download and replacement; retain app ownership
of UI, restart coordination and recovery. No update server or installer is required.
Automatic detection should not mean unsolicited installation or restart.

## Verified current release process

The local checkout was clean at investigation start (HEAD `a5c83d9`). Both workflow
files have identical blob hashes locally and on GitHub's default branch, `main`.
The repository is public.

- `.github/workflows/release.yml` is manually dispatched after bumping the package
  version in `Cargo.toml`. It rejects an already-published version, calls the build
  workflow, then creates `v<version>` at the workflow SHA with generated notes.
- `.github/workflows/build.yml` also runs on main pushes, pull requests and manual
  dispatch. It tests and builds with Rust 1.99.0, `--release --locked` and
  `RUSTFLAGS=-Dwarnings`. Release calls bypass the Rust cache.
- Targets: `x86_64-pc-windows-msvc` on Windows 2022 and
  `x86_64-unknown-linux-gnu` on Ubuntu 24.04. These are x64 builds only.
- Each archive contains the executable, LICENSE, README and INSTALL instructions.
  There is no installer, AppImage, DEB/RPM or updater feed.
- Latest published release is [v0.2.0](https://github.com/ricnewton/tokenkeeper/releases/tag/v0.2.0),
  published 10 November 2026 at 19:28 UTC from `431c294dfdb13288a44845537bd8dbd29665efcc`.
  Its [release workflow succeeded](https://github.com/ricnewton/tokenkeeper/actions/runs/42884885102).
  Assets: `tokenkeeper-win-0.2.0.zip` (371,404 bytes) and
  `tokenkeeper-linux-0.2.0.tar.gz` (4,002,088 bytes).
- The GitHub API reports SHA-256 asset digests and `immutable: false`. Current
  archives are unsigned; there are no update signing steps in CI.

Creating a release manually in GitHub does not trigger these builds. The existing
workflow creates the release after building. An updater must require complete,
correctly signed platform assets, not merely the existence of a newer tag.

## Application integration

`Cargo.toml` is currently version 0.2.0. This is native Rust: Win32/Direct2D on
Windows and Iced/ksni on Linux, not Electron or Tauri. `docs/design/deployment.md`
already identifies update checking as an outstanding feature.

Suggested implementation:

1. Add a shared updater module with semantic-version comparison and explicit
   states: checking, available, downloading, verifying, ready, failed.
2. Check in a separate background worker after startup and roughly daily, with
   jitter, persisted last-check time and backoff. Add **Check for updates** to
   both tray menus. Never block provider polling or UI on update I/O.
3. Show a persistent, dismissible panel message and tray action with the new
   version and release notes link. Clicking **Update and restart** authorizes
   download and restart; show progress and useful errors.
4. Select only the appropriate platform asset, verify version, signature and
   bounded size, stage locally, then replace and restart. Keep a recovery copy.
   Ignore prereleases by default and never automatically downgrade.

Integration points: `src/linux_app.rs`, `src/linux_tray.rs`, `src/linux_panel.rs`,
`src/main_windows.rs`, `src/popup.rs`, `src/main.rs` and `src/instance.rs`.
The provider worker in `src/worker.rs` demonstrates background event delivery but
does not currently offer an explicit shutdown-and-join protocol. Define shutdown
coordination before restarting to avoid interrupting credential/history writes.

Important existing constraints:

- `src/http_linux.rs` disables redirects and has a 15-second global timeout.
  Both HTTP implementations return buffered bodies. GitHub downloads need a
  separate bounded streaming path, suitable timeouts and safe HTTPS redirects.
  Do not weaken provider HTTP policies or send provider credentials to GitHub.
- Windows uses WinHTTP specifically for system proxies and certificate trust;
  Linux uses ureq with platform verification and explicit proxy handling. Any
  library HTTP client must preserve corporate proxy/TLS inspection compatibility.
- `main()` holds a single-instance mutex/flock until exit. A naive spawn-then-exit
  restart can cause the new process to reject itself as a duplicate. Use a tested
  handoff that waits for the old process to exit before normal initialization.
- Autostart tracks the executable path. Replace at the existing installed path;
  do not launch a downloaded temporary copy as the normal app.
- Credentials and data live separately (`%APPDATA%/ai-usage`, Linux XDG storage
  and Secret Service). Executable replacement should preserve them. Future data
  migrations must also be compatible with the chosen recovery strategy.

## Platform behavior

| Platform | Proposed behavior | Boundary |
| --- | --- | --- |
| Windows | Replace in the user-writable installation folder; restart after shutdown | Running EXE cannot be overwritten normally; use proven replacement machinery. Protected folders and antivirus locks need actionable errors. |
| Linux | Stage beside the installed executable, preserve executable permissions and atomically rename into place | Support the documented `~/.local/bin` installation. Do not self-update root-owned or package-manager-owned files. |

[`self-replace`](https://docs.rs/self-replace/latest/self_replace/) implements
Unix atomic replacement and Windows rename/helper cleanup. It supplies replacement,
not a complete recovery or application lifecycle system. Automatic rollback after
a failed launch would require a separate health-check handshake; retaining a backup
alone does not provide that guarantee.

Updating does not broaden Linux compatibility: the current Ubuntu 24.04 GNU build
still needs a compatible runtime and the documented desktop libraries. Test actual
supported distributions; do not advertise arbitrary Linux support from an updater.

## Library choices

[`self_update`](https://docs.rs/self_update/latest/self_update/) supports GitHub,
ZIP/tar.gz, checks without installing, optional checksums and zipsign archive
signatures. Disable terminal prompts/output for GUI use. Configure exact asset and
archive-member selection because current filenames use `win`/`linux`, not Rust
target triples. Require compiled-in nonempty verification keys; an empty key list
disables [signature verification](https://docs.rs/self_update/latest/self_update/update/fn.verify_signature.html).
Validate dependency size and network behavior in a small prototype before adopting.

Fallback: a narrow downloader plus `self-replace` if retaining the current Windows
WinHTTP stack outweighs the extra updater logic we would then own.

[Velopack supports Rust](https://docs.velopack.io/getting-started/rust), but requires
its packaging layout and release metadata. Its [Linux implementation uses AppImage](https://docs.velopack.io/packaging/operating-systems/linux).
That is a reasonable future distribution change, but unnecessary for the existing
single-file installation model. Do not introduce a new UI framework for updating.

## GitHub and signing changes

Use GitHub's [latest release API](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)
without authentication for this public repository. Compare the release version with
the running binary instead of assuming GitHub's latest designation means newer.
Public API limits are [60 unauthenticated requests per hour per IP](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api);
cache checks, respect rate-limit responses and avoid requiring users to supply tokens.

Before distributing executable updates:

- Generate an update-signing key and embed its public key in the application.
  Store the private key as a protected release secret, unavailable to PR builds.
- Sign each completed archive before upload, then verify signatures in CI.
  Plan key rotation with an overlapping trusted-key release.
- Publish as a draft, attach and validate both signed assets, then publish as
  latest. This keeps clients from observing an incomplete release.
- Consider enabling immutable releases after reviewing its constraints.
  [GitHub recommends uploading draft assets before publication](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository).
- A hash served alongside an asset detects corruption but is not independent
  publisher authentication. Protect signing access as well as the repository.
- Update signatures are separate from Windows Authenticode. They do not remove
  the existing SmartScreen warnings; Windows code signing is a separate decision.

## Verification and delivery scope

Start with a bounded integration spike: prove a Windows and Linux A-to-B update,
including signature checks, corporate networking behavior, archive selection and
single-instance restart. Then implement UI, scheduling, release signing and recovery.

Required evidence before shipping:

- Older/equal/newer/prerelease comparisons and exact platform selection.
- Tampered, unsigned, truncated and wrong-platform assets rejected before replace.
- Offline, rate-limited and interrupted downloads leave the current app usable.
- Read-only destination, disk-full, Windows antivirus locks and interrupted
  replacement produce recoverable outcomes.
- Exactly one app after restart; existing accounts, history and autostart retained.
- Real installed A-to-B update on Windows and Linux, not just unit tests or builds.

Planning estimate, not a measured commitment: notification-only is roughly 1–2
developer days; a tested signed one-click updater for both platforms roughly 5–10
developer days total, with Windows recovery and network compatibility the main
uncertainties. Automatic failed-launch rollback and Authenticode rollout add scope.

Existing v0.2.0 installations cannot learn this feature remotely. Users must install
the first updater-capable release manually once; subsequent releases can use it.

No runtime update prototype or installation test was performed in this research.
Only this research note was added; production code and workflows are unchanged.


## Implementation record — 12 November 2026

Implemented on `codex/automatic-updates`, using the smallest shared controller
around `self_update` 1.3.3. The tray menu and tooltip carry update status; there
is no new window, installer, progress percentage, persistent scheduler or rollback
service. Checks run after startup and daily while the app stays open.

Completion contract: exact platform/version selection, signed download rejection
before replacement, user-triggered install, background network work, safe provider
shutdown and single-instance restart on Windows/Linux. Existing storage and
installation paths remain unchanged. Verify using the suite, signed fixture
integration tests, release builds, native GitHub CI and isolated replacement smoke.

The public key is committed. The generated private key is stored only under
`.local/update-signing/private.key` (mode 600), and was configured as the repository
Actions secret with explicit user approval. Future key rotation must preserve
trust for already-installed versions. No release has been published by this work.

Independent review identified an interrupted credential-save risk during restart.
Provider workers now disconnect their command channels and finish active work
before the UI restarts. A regression test holds a request open across shutdown.
Restart failures leave a retry action; the provider workers stay stopped until
restart, rather than resuming writes after the binary has changed.

The updater uses ureq and system certificate verification. Windows automatic
WinHTTP proxy discovery is not reused; proxy environment settings are documented.
Simplicity tradeoffs: no failed-launch rollback, no automatic elevation, no changes
to Linux runtime compatibility. See the permanent setup and signing guides.


Verification so far: 62 Linux tests passed, 2 existing desktop-dependent tests
ignored. Signed fixture tests prove replacement and rejection of tampered,
unsigned and wrong-name downloads. The isolated Linux subprocess smoke test
replaced a running old harness with a different signed executable and restarted;
new bytes and single-instance lock acquisition were verified. Script/evidence
are under `.local/update-smoke/`. It uses mocked HTTP and does not exercise the GUI.
Native GitHub Windows/Linux verification runs on the branch. Initial Windows CI
caught a Linux-only test helper compiled on Windows; its cfg was corrected.

At the user's request, an AES-256 GnuPG encrypted backup was copied to Percy and
verified by checksum. A Bitwarden secure-note JSON import was prepared locally;
the user must import it and confirm success before deleting that plaintext file.
Backup locations and secrets remain in private recovery material, not Git.


Final verification: [native GitHub CI run 43272429035](https://github.com/ricnewton/tokenkeeper/actions/runs/43272429035)
passed Windows x64 and Linux x64 tests, release builds and archive packaging for
code revision `e0ba0fc`. Local `RUSTFLAGS=-Dwarnings cargo test --release --locked`
also passed 62 tests with the two existing desktop-only tests ignored. Changes
after that revision only document verification and backup/recovery guidance.
Windows in-place replacement/restart and the GUI were not manually exercised;
Windows signed-download/replacement fixtures ran in native CI, and actual Linux
self-replacement/restart was exercised by the isolated harness. The branch is
ready for review; no merge or release was performed.
