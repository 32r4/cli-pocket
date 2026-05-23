# Releasing cli-pocket

This is a manual process. CI does the heavy lifting (build + sign + draft
release), but a maintainer must approve and publish.

## Prerequisites (one-time)

- The maintainer has access to the GitHub repo `32r4/cli-pocket`.
- GitHub Actions secrets `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD` are set.
- Local minisign public key matches `docs/release/PUBLIC_KEY.md`.

## Steps

1. Decide the new version, e.g. `0.2.0`. Follow semver.
2. Update `CHANGELOG.md` (manual; auto-generation is out of scope for v1).
3. Bump versions in:
   - All `crates/**/Cargo.toml` (use `cargo set-version 0.2.0 --workspace`).
   - `apps/desktop/src-tauri/tauri.conf.json`, `apps/mobile/src-tauri/tauri.conf.json`.
   - `apps/web/package.json`, `webview/terminal/package.json`.
4. Commit: `git commit -am "chore: bump to 0.2.0"`.
5. Tag: `git tag -a v0.2.0 -m "v0.2.0"`.
6. Push: `git push && git push --tags`. CI runs the `release` workflow.
7. Watch the workflow; on success it creates a **draft** release with all
   artifacts attached.
   - Android/iOS jobs run only when `apps/mobile/src-tauri/tauri.conf.json`
     exists in the tagged revision.
8. Download `SHA256SUMS` and `SHA256SUMS.minisig`; verify locally:

   ```
   minisign -V -p cli-pocket-minisign.pub -m SHA256SUMS
   ```

9. Update the Homebrew tap (`32r4/homebrew-cli-pocket`):
   - Take `packaging/homebrew/cli-pocket.rb.template`.
   - Substitute `@VERSION@` and the four `@SHA_*@` values from `SHA256SUMS`.
   - Open a PR against the tap repo.
10. Edit the draft release notes (paste the relevant chunk of `CHANGELOG.md`),
    then **Publish release**.
11. Announce: project README badge auto-tracks the latest release; no other
    announcement system is wired up in v1.

## Rollback

If a release is broken:

1. Click **Delete release** on the GitHub release page.
2. Delete the tag locally: `git tag -d v0.2.0 && git push origin :refs/tags/v0.2.0`.
3. The Homebrew tap PR can be closed (or merged then reverted).
