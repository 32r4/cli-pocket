# Plan J Handoff: Release Pipeline

Date completed: 2026-05-24
Implementer: Codex orchestrator

## What shipped

### Workflow

- `.github/workflows/release.yml` now uses explicit matrix flags instead of
  `contains(matrix.kinds, ...)` string matching.
- The build matrix covers:
  - Linux x86_64
  - Linux aarch64
  - macOS x86_64
  - macOS aarch64
  - Windows x86_64
  - Android
- The publish job now extracts the minisign public key from the checked-in
  `docs/release/PUBLIC_KEY.md` instead of fetching/parsing `main` over HTTP.

### Release scripts

- Existing scripts kept and hardened:
  - `scripts/release/build-daemon.sh`
  - `scripts/release/build-relay.sh`
  - `scripts/release/build-desktop.sh`
  - `scripts/release/build-web.sh`
  - `scripts/release/sign.sh`
  - `scripts/release/verify.sh`
  - `scripts/release/sha256sums.sh`
- Added:
  - `scripts/release/build-mobile-android.sh`
  - `scripts/release/build-mobile-ios.sh`
  - `scripts/release/extract-public-key.sh`

### Behavior changes

- Windows archive creation for daemon/relay artifacts falls back to
  `powershell.exe Compress-Archive` if `zip` is unavailable.
- `sha256sums.sh` works across environments by trying:
  - `sha256sum`
  - `shasum -a 256`
  - PowerShell `Get-FileHash`
- `verify.sh` now fails clearly when no `.minisig` files exist.
- Mobile release scripts skip cleanly when `apps/mobile/src-tauri/tauri.conf.json`
  is absent, instead of pretending a build happened.

## Deviations from plan / current limitations

- `docs/release/PUBLIC_KEY.md` still contains placeholder key material. The
  updated workflow now fails loudly at key extraction time until a real public
  key is committed.
- Full GitHub Actions matrix execution was not run from this local environment.
  Static correctness improved, but cross-platform release proof still requires
  a real tag push / workflow run.
- iOS and Android artifact generation still depends on SDK-provisioned runners
  plus the mobile app's generated Tauri project assets.

## Commands

- `bash -n scripts/release/build-daemon.sh`
- `bash -n scripts/release/build-relay.sh`
- `bash -n scripts/release/build-desktop.sh`
- `bash -n scripts/release/build-web.sh`
- `bash -n scripts/release/build-mobile-android.sh`
- `bash -n scripts/release/build-mobile-ios.sh`
- `bash -n scripts/release/sign.sh`
- `bash -n scripts/release/verify.sh`
- `bash -n scripts/release/sha256sums.sh`
- `bash -n scripts/release/extract-public-key.sh`

## Validation

- Shell syntax checks for all release scripts - passed.
- `bash scripts/release/sha256sums.sh <temp-dir>` - passed.
- `bash scripts/release/extract-public-key.sh docs/release/PUBLIC_KEY.md ...`
  - fails intentionally while the public key doc still contains placeholders.
- `just check` passed after H/J aggregation in the integrated worktree.

## Follow-ups

- Replace placeholder content in `docs/release/PUBLIC_KEY.md` before the first
  real signed release.
- Run a real `v*` tag through GitHub Actions to validate platform-specific
  packaging, signing, and artifact upload behavior.
- If mobile release artifacts become mandatory rather than optional, enforce
  generated Android/iOS project assets instead of skipping when absent.
