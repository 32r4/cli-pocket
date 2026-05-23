# 0004. Self-hosted Rust relay with trait abstraction

Date: 2026-05-21
Status: Accepted
Owners: cli-pocket

## Context

We need a relay so phones (NAT, hotel Wi-Fi) can reach desktops without
port-forwarding. The two realistic shapes are:

1. Self-hosted Rust binary you run on a $5/mo VPS.
2. Cloudflare Durable Objects (or similar serverless) for zero ops.

## Decision

Ship (1) as the v1 default. Keep the relay surface narrow enough that a CF DO
implementation could be added later behind the same `RelayCtrl` / `RelayData`
contract.

## Consequences

- Positive: zero vendor lock-in, deployable anywhere, easy local CI testing.
- Positive: relay sees only ciphertext; compromise = traffic-analysis only.
- Negative: each user must run/operate the relay (or trust a community one).
- Risk accepted: spam offers DoS — mitigated by per-host pair caps and
  by hosts only accepting offers from paired client public keys.
