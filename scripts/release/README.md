# scripts/release/

Per-target build scripts. Each script:

- Takes `$TARGET` (e.g. `x86_64-unknown-linux-gnu`) and `$VERSION` (semver
  without leading `v`) from the environment.
- Produces one tarball in `dist/` whose name is
  `<artifact>-<version>-<target>.<ext>` (e.g.
  `cli-pocket-daemon-0.1.0-x86_64-unknown-linux-gnu.tar.gz`).
- Exits non-zero on failure.

After all scripts run, `sign.sh dist/` produces a `.minisig` next to each file
and `sha256sums.sh dist/` produces `SHA256SUMS` and `SHA256SUMS.minisig`.

Run locally:

```sh
VERSION=0.1.0 TARGET=x86_64-unknown-linux-gnu bash scripts/release/build-daemon.sh
VERSION=0.1.0 bash scripts/release/sign.sh dist/
```
