# Security Policy

cli-pocket handles end-to-end encryption (Noise XK) and a PAKE-based pairing
flow (SPAKE2). Cryptographic bugs and protocol flaws are taken seriously.

## Supported Versions

Pre-1.0: only `main` is supported. Once a tagged release exists, the latest
minor will be supported.

## Reporting a Vulnerability

**Do not open a public GitHub issue for security problems.**

Use GitHub's private vulnerability reporting:

1. Open https://github.com/32r4/cli-pocket/security/advisories/new
2. Describe the issue, the affected component (daemon, relay, client, proto,
   crypto), and a proof of concept if you have one.

We aim to acknowledge reports within 5 business days. Coordinated disclosure
window is 90 days unless the issue is being actively exploited, in which case
we'll work with you on a faster timeline.

## Scope

In scope:

- The daemon, relay, and client code in this repository.
- The wire protocol, Noise handshake, and SPAKE2 pairing flow.
- Identity / key persistence and revocation behavior.

Out of scope:

- Vulnerabilities in upstream dependencies - please report those upstream
  (we'll cut a patch release once an upstream fix is available).
- Social engineering of repository maintainers.
- Denial of service against a relay you do not own.

## Hall of Fame

Researchers who report valid issues will be credited in release notes if they
wish.
