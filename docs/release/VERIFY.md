# Verifying a cli-pocket release

Every release at https://github.com/32r4/cli-pocket/releases includes
`SHA256SUMS`.

For all releases:

1. Download `SHA256SUMS` and the artifact you want to install.
2. Run:

   ```sh
   sha256sum -c SHA256SUMS
   ```

If verification fails, do not install. Open an issue at
https://github.com/32r4/cli-pocket/issues.
