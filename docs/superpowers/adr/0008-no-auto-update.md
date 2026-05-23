# ADR 0008: no auto-update mechanism in v1

Status: Accepted

## Context

Desktop apps, mobile apps, and CLI binaries commonly ship an auto-update
mechanism that polls a manifest, downloads a new build, verifies it, and
restarts. Implementing this correctly requires: a signed update manifest
format, per-platform installer hand-off (MSI/DMG/AppImage/APK), background
download UX, rollback, and a server endpoint or static host for manifests.
Each of these has security and operational implications that we do not want
to take on for the v1 release of cli-pocket.

## Decision

Ship no auto-update mechanism in v1. Users update by re-downloading the
latest release from GitHub Releases and re-running the installer (desktop /
mobile) or replacing the binary (CLI / daemon / relay). The README and
`docs/release/VERIFY.md` instruct users how to check signatures on the new
artifact before installing.

## Consequences

- Smaller attack surface: no update endpoint, no manifest server, no
  background network activity in the binaries.
- Users on stale versions are not nudged to upgrade; security fixes propagate
  only as fast as users notice releases.
- When v2 reconsiders auto-update, it MUST reuse the minisign verification
  path from ADR 0007: ship update manifests signed by the same key, verify
  before applying, and refuse downgrades.
- Package managers (apt/dnf/Homebrew) provide their own update path for the
  users they reach; nothing in this ADR blocks that.
