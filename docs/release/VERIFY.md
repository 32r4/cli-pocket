# Verifying a cli-pocket release

Every artifact at https://github.com/32r4/cli-pocket/releases has a matching
`.minisig` file. To verify:

1. Download the public key from `docs/release/PUBLIC_KEY.md`.
2. Save it to `cli-pocket-minisign.pub`.
3. Run:

   ```
   minisign -V -p cli-pocket-minisign.pub -m <file>
   ```

If verification fails, do not install. Open an issue at
https://github.com/32r4/cli-pocket/issues.
