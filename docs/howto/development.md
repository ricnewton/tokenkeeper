# Development and screenshots

Linux screenshots can be rendered without signing in or opening a desktop window:

```bash
mkdir -p .local/previews
target/release/tokenkeeper --demo-sheet .local/previews/tokenkeeper-dark.png --demo-provider codex --demo-tab 0 --demo-single
TOKENKEEPER_THEME=light target/release/tokenkeeper --demo-sheet .local/previews/tokenkeeper-light.png --demo-provider codex --demo-tab 0 --demo-single
target/release/tokenkeeper --demo-sheet .local/previews/tokenkeeper-providers.png --demo-provider cursor --demo-tab 1
```

`--demo` opens a sample panel without accessing credentials or changing autostart.
Windows provides visual previews in both themes:

```powershell
New-Item -ItemType Directory -Force .local\previews | Out-Null
.\target\release\tokenkeeper.exe --icon-sheet .local\previews\icons.png
.\target\release\tokenkeeper.exe --popup-sheet .local\previews\popups.png
```

Inspect the generated PNG before relaunching to check layout changes.

Store local previews, downloads, logs and recovery backups under `.local/`,
which is ignored by Git. Avoid the system temporary directory for these files.

## Build checks

Keep both platforms warning-free. On Linux:

```bash
RUSTFLAGS=-Dwarnings cargo test --release --locked
RUSTFLAGS=-Dwarnings cargo build --release --locked
```

With the Windows GNU Rust target and its toolchain installed, also check the
Windows source:

```bash
RUSTFLAGS=-Dwarnings cargo check --locked --target x86_64-pc-windows-gnu
```

A cross-check verifies compilation; test tray interactions on Windows itself.
Quit the running Windows copy before rebuilding because Windows locks the executable.
See [deployment](../design/deployment.md) for installation and update steps.

## GitHub Actions

The **Build and test** workflow runs on pushes to `main`, pull requests and
manual dispatch. A single workflow has two jobs from a matrix: Ubuntu 24.04
and Windows Server 2022. Both run warning-free release tests and builds using
Rust 1.99.0. Windows builds natively with MSVC so its tests actually run;
the local GNU cross-check above is an additional compilation check.

Open a successful run in the repository's **Actions** tab and download
`tokenkeeper-win-<branch>-<run>.zip` or `tokenkeeper-linux-<branch>-<run>.tar.gz`,
where `<run>` is the workflow run number. Each download is the archive itself
(uploaded with `archive: false`, so GitHub does not wrap it in another zip). It
contains the executable, README, installation instructions and MIT license.
Artifacts expire after seven days. Linux builds
require the desktop runtime libraries listed in the installation instructions;
they are not static binaries. Windows binaries are unsigned.

The jobs run without supplier credentials or interactive sign-in. Unit tests
and builds do not validate live account access or tray behavior; those still
need desktop testing. The build workflow needs only repository read permission and
works with a private repository on GitHub Free, within its included allowance.

### Releases

To publish a release, bump `version` in `Cargo.toml` on `main`, push it, then
run the **Release** workflow from the Actions tab. It reads the version, fails
if release `v<version>` already exists, runs the same build and tests (by
calling `build.yml`), then creates tag `v<version>` on that commit and a GitHub
release with generated notes and two assets: `tokenkeeper-win-<version>.zip`
and `tokenkeeper-linux-<version>.tar.gz`. Only the publish job has write
permission to the repository contents.
