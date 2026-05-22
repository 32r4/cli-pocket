# Handoff - Plan C (PTY + scrollback)

Date completed: 2026-05-22
Implementer: Codex

## What was built

### `cli-pocket-pty` (0.1.0)

Public API surface, re-exported from `crates/server/pty/src/lib.rs`:

- `Terminal`, `TerminalError`
- `ScrollbackRing`, `RingError`
- `AnchorTracker`
- `OutputBroadcaster`, `OutputChunk`, `OutputRecv`, `OutputStream`, `Lagged`

### `Terminal`

`crates/server/pty/src/terminal.rs` provides:

- `Terminal::spawn(&TerminalCreateParams) -> Result<Terminal, TerminalError>`
- `id() -> TerminalId`
- `dims() -> (u16, u16)`
- `head_seq() -> StreamSeq`
- `write_input(&[u8]) -> Result<(), TerminalError>`
- `subscribe() -> OutputStream`
- `snapshot() -> Snapshot`
- `since(StreamSeq) -> Option<DeltaSlice>`
- `resize(cols, rows) -> Result<(), TerminalError>`
- `kill(KillSignal) -> Result<(), TerminalError>`
- `wait().await -> ExitInfo`

Implementation notes:

- Uses `portable-pty` 0.8 via `native_pty_system()`.
- `spawn()` opens the PTY, resolves a default shell when `cmd` is empty, spawns the child, and starts:
  - a reader thread that drains PTY output into `ScrollbackRing` and `OutputBroadcaster`
  - a waiter thread that blocks on `Child::wait()` and publishes `ExitInfo` via `tokio::sync::watch`
- Default shell resolution lives in `crates/server/pty/src/platform/`:
  - Unix: `$SHELL`, else `/bin/sh`
  - Windows: `C:\Windows\System32\cmd.exe`, else `powershell.exe`

### `ScrollbackRing`

`crates/server/pty/src/ring.rs` implements a bytes-plus-anchor ring:

- Default capacity: 4 MiB
- Max capacity: 64 MiB
- Anchor interval target: 64 KiB
- State:
  - retained bytes in `VecDeque<u8>`
  - anchor list in `VecDeque<Anchor>`
  - `head_seq` / `tail_seq`
  - current terminal dimensions
  - inline `AnchorTracker`

Semantics:

- `push()` advances the parser one byte at a time, appends to the byte ring, advances `head_seq`, tries to place a new anchor, then evicts if over capacity.
- `snapshot()` returns bytes from the oldest retained anchor through `head_seq`, plus that anchor's `AnchorState`.
- `since(seq)` returns `None` if `seq < tail_seq` or `seq > head_seq`, otherwise the retained suffix from `seq` through `head_seq`.
- Eviction drops to the second anchor boundary, so the oldest retained byte is always aligned to the oldest retained anchor.

### `AnchorTracker`

`crates/server/pty/src/parser.rs` tracks the parser state needed for anchors:

- cursor row/column
- SGR attributes, including palette / indexed / RGB colors
- terminal modes (`DECCKM`, autowrap, alt-screen, bracketed paste, mouse modes, origin mode)
- OSC 0 / OSC 2 title
- charset designations for G0-G3 final bytes

Safe-split behavior:

- The tracker starts at a safe split.
- `print`, `execute`, `unhook`, `osc_dispatch`, `csi_dispatch`, and handled `esc_dispatch` end in a safe split.
- Partial escape / hook / put states are not safe splits.
- The ring only places a new anchor once the anchor interval is reached and the tracker reports a safe split.
- If no later safe split arrives, the ring keeps the last safe anchor instead of forcing a split mid-sequence.

### Output stream

`crates/server/pty/src/output.rs` wraps `tokio::sync::broadcast`:

- broadcaster capacity is 1024 chunks
- subscribers call `OutputStream::recv().await`
- slow subscribers receive `OutputRecv::Lagged { skipped }`
- the PTY drain never blocks on lagging subscribers

### ADR / lint boundary

- `docs/superpowers/adr/0002-bytes-plus-anchor-scrollback.md` records the bytes-plus-anchor decision.
- `crates/server/pty/Cargo.toml` locally relaxes `unsafe_code = "allow"` for this crate only, keeping any future PTY-specific unsafe boundary isolated here.

## Deviations from Plan C / spec

- `Terminal::spawn` takes `&TerminalCreateParams`, not an owned `TerminalCreateParams`.
- `Terminal` exposes `OutputBroadcaster`, `OutputChunk`, and `OutputRecv` publicly in addition to the smaller handoff-template surface.
- Exit handling is implemented with a blocking `Child::wait()` thread plus `tokio::sync::watch`, not a polling `try_wait()` loop.
- `kill(KillSignal)` currently ignores the specific signal variant and always uses `portable-pty`'s single `kill()` primitive.
- `ExitInfo.signal` is always `None`. `portable-pty` 0.8.x does not expose a stable numeric signal accessor, and the implementation intentionally refuses to parse display text into invented signal numbers.
- Safe splits are stricter than the original plan text's "or 2x interval if no safe split arrives sooner" fallback. The merged code does not force a mid-sequence split; it retains the last safe anchor until the parser reaches a safe boundary again. That can temporarily keep more than the nominal capacity during a long unterminated control sequence.
- Charset tracking is partial. The parser records G0-G3 designation final bytes from handled SCS `ESC ( ) * +` sequences, but does not track GL/GR shifts beyond the proto defaults.
- Cursor advancement still treats non-control characters as width 1. That is an approximation for anchors, not a cell-accurate renderer.

## Open questions / follow-ups

- If Plan D needs real signal semantics (`Hup` vs `Term` vs `Kill`), `cli-pocket-pty` will need platform-specific handling beyond `portable-pty` 0.8's generic kill path.
- The safe-split policy is conservative and avoids mid-sequence anchors, but a very long unterminated OSC/DCS stream can keep the oldest safe anchor pinned and let retained bytes exceed nominal capacity until a safe boundary returns.
- Wide-character and combining-character cursor tracking is approximate. xterm.js remains the renderer of record, but anchor cursor state may be off for CJK / combining-heavy output.
- Charset state is incomplete for full ECMA-35 behavior. `g[]` designations are tracked, but active shift state is not fully modeled.
- Upstream handoffs already note a local `cargo-deny` caveat: older local `cargo-deny` 0.17.0 builds can misparse `deny.toml`. This plan is docs-only and did not rerun that gate, but downstream validation should rely on the pinned Plan A / CI toolchain rather than that older local version.

## Validation

- Docs-only task: no code tests run.
- Required scans run locally:
  - placeholder-token scan on the new handoff
  - unfinished-marker scan on the new handoff
- `git status` verified only `docs/superpowers/handoff/C.md` changed before staging.
