# scripts/release/

Per-target build scripts. Each script:

- Takes `$TARGET` (e.g. `x86_64-unknown-linux-gnu`) and `$VERSION` (semver
  without leading `v`) from the environment.
- Produces one tarball in `dist/` whose name is
  `<artifact>-<version>-<target>.<ext>` (e.g.
  `cli-pocket-daemon-0.1.0-x86_64-unknown-linux-gnu.tar.gz`).
- Exits non-zero on failure.

After all scripts run, `sha256sums.sh dist/` produces `SHA256SUMS`.

Mobile scripts expect a real `apps/mobile/src-tauri/tauri.conf.json`. If the
mobile app scaffold has not landed yet, they exit early with a skip message
instead of pretending to have built artifacts.

Run locally:

```sh
VERSION=0.1.0 TARGET=x86_64-unknown-linux-gnu bash scripts/release/build-daemon.sh
bash scripts/release/sha256sums.sh dist/
```
