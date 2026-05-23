# cli-pocket release signing public key

The public key below verifies every signed release artifact under
`https://github.com/32r4/cli-pocket/releases`.

```text
untrusted comment: cli-pocket release key
<paste public key from cli-pocket-minisign.pub here>
```

Fingerprint: `<minisign key id hex>`

The first time you install cli-pocket, copy this key to
`~/.config/cli-pocket/minisign.pub` and run `cli-pocket verify <file>` against
each downloaded artifact — or use `minisign -V -p <key> -m <file>` directly.
