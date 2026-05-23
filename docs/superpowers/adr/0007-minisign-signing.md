# ADR 0007: minisign signing for release artifacts

Status: Accepted

## Context

cli-pocket ships native binaries, installers, and a web bundle on every
release. Users download these artifacts from GitHub Releases and must be able
to verify they were produced by the project maintainers and not tampered with
in transit or on the release page. The signing tool must be simple, scriptable
in CI, available on Linux/macOS/Windows, and produce signatures that an
end-user can verify without GPG-style keyring setup.

## Decision

Use `minisign` (Ed25519 over BLAKE2b) to sign every release artifact. One
signing keypair is generated once on a maintainer machine and never lives in
the repo: the secret key and its passphrase are stored as GitHub Actions
secrets (`MINISIGN_SECRET_KEY`, `MINISIGN_PASSWORD`). The corresponding public
key is committed to the repo under `docs/release/PUBLIC_KEY.md`. CI signs each
artifact and the aggregate `SHA256SUMS` file; end users verify with
`minisign -V -p cli-pocket-minisign.pub -m <file>`.

## Consequences

- Single small dependency (`minisign`) on developer and CI machines; available
  via apt, brew, and chocolatey.
- No GPG keyring, no web of trust, no key servers; the trust anchor is the
  public key file in the repo.
- Key rotation is a manual procedure documented in
  `docs/release/KEY_ROTATION.md`; rotations require a repo PR plus updating
  CI secrets.
- macOS Gatekeeper and Windows SmartScreen are independent of minisign; Apple
  notarization and Authenticode signing are optional and tracked separately.
