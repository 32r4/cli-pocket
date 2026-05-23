# Signing-key rotation

1. Generate a new keypair: `minisign -G -p new.pub -s new.key`.
2. Open a PR that updates `docs/release/PUBLIC_KEY.md` with the new pubkey,
   keeping the old key under a "Previous keys" section for 90 days.
3. Merge.
4. Update GitHub Actions secrets `MINISIGN_SECRET_KEY` / `MINISIGN_PASSWORD`.
5. Tag the next release; CI signs with the new key.
6. After 90 days, delete the old key from the docs.
