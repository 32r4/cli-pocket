# 0003. Noise XK over JSON+TLS-only trust model

Date: 2026-05-22
Status: Accepted
Owners: Codex

## Context

We needed end-to-end mutual authentication between the daemon and each
client, surviving a fully malicious relay. The alternatives were:

1. TLS only (mTLS) — relies on the public CA system or on issuing client
   certs out of band. Browsers also won't silently accept self-signed certs
   for LAN endpoints.
2. JSON message signatures over TLS — re-invents authenticated transport;
   nonce handling and replay are easy to get wrong.
3. Noise Protocol XK pattern over a plain transport (WS/TCP).

## Decision

Use Noise_XK_25519_ChaChaPoly_BLAKE2s (via the `snow` crate) as the
end-to-end channel. TLS, where present (relay, future web exposure), is
purely transport hygiene — it lets `wss://` survive proxies and corporate
networks. The trust model is Noise, end-to-end. A compromised TLS layer
cannot read or forge frames; it sees Noise ciphertext.

XK specifically because:
- K: the client knows the daemon's static public key from pairing.
- X: the daemon learns the client's static key during the handshake and
  checks it against `clients.json` for authorization.

A PSK option (XKpsk2) is available for self-hosted relays that want to gate
relay-level access. PSK is **not** part of the daemon-client trust model.

## Consequences

- Positive: the relay is forced to be zero-trust at the protocol level.
  Operators can run a relay without being trusted by their users.
- Positive: a stolen TLS cert (or a rogue corporate proxy) cannot decrypt
  terminal sessions.
- Positive: well-audited primitives; we never roll our own AEAD or KDF.
- Negative: the handshake is 3 roundtrips, adding ~1–2 RTT to the
  cold-connect time compared to immediate TLS app-data.
- Negative: revoking a client requires editing `clients.json` and waiting
  for the file watcher to pick up the change (Plan D detail). There is no
  cryptographic short-circuit revocation in v1.
- Risks accepted: side-channel attacks against `snow` / `spake2`. We rely
  on upstream review and do not roll our own primitives.
