# 0002. Bytes-plus-anchor scrollback rather than parsed grid

Date: 2026-05-22
Status: Accepted
Owners: Codex

## Context

The PTY layer needs to let late-attaching clients resume rendering from the
last retained terminal state plus subsequent bytes. Two designs were on the
table:

1. Parsed grid, where the daemon maintains a full cell-level terminal model.
2. Bytes plus anchor, where the daemon stores raw output bytes and a parser
   snapshot at safe split points, then lets the client re-feed the bytes into
   its own renderer.

The PTY crate also needs a per-crate `unsafe_code` relaxation. The workspace
defaults to `unsafe_code = "forbid"`, but `portable-pty`'s Windows plumbing
may require unsafe internals later. We are not using unsafe code today; the
override avoids a future lint conflict when the Windows path is added.
This crate is the safety boundary for PTY-specific concerns. Higher layers
consume safe Rust types only; any future raw handle or ConPTY work stays
contained here.

## Decision

Use bytes-plus-anchor scrollback. The daemon stores raw PTY bytes and an
`AnchorState` captured at safe parser boundaries. The client remains the
canonical renderer and replays bytes from the chosen anchor.

Relax `unsafe_code` to `allow` only in `cli-pocket-pty`.

## Consequences

- Positive: no daemon-side vt100 grid implementation to maintain.
- Positive: snapshot and delta stay byte-oriented and cheap to store.
- Positive: the client renderer remains the source of truth for display.
- Negative: the daemon does not expose a queryable terminal cell grid.
- Negative: cursor and mode state must be tracked carefully at anchors.
- Risks accepted: future Windows PTY code may need unsafe internals, and the
  crate-level lint allows that without widening the workspace policy.
