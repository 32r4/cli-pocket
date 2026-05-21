# cli-pocket Overview Plan

> **For agentic workers:** This is a coordination plan, not an executable task list. It defines how the 10 sub-plans (A–J) relate, what order they run in, and the project-wide mechanisms (ADRs, proto freeze, handoff notes). The actual buildable tasks live in the per-area plans listed below.

**Goal:** Coordinate the implementation of the cross-platform remote terminal across 10 dependency-ordered sub-plans, so they can be picked up sequentially or partially in parallel without losing track of contracts, hand-offs, or validation gates.

**Spec:** `docs/superpowers/specs/2026-05-21-cross-platform-remote-terminal-design.md`

**Architecture:** Rust workspace with four role groups (`shared/`, `server/`, `relay/`, `client/`), three Tauri/Web app packages, one shared webview Vite project, and one published wasm binding. Plans are sequenced by dependency layer, not by directory.

---

## Sub-Plans

| ID | Title | File (when written) | Depends on | Can run in parallel with |
|---|---|---|---|---|
| A | Scaffold + CI baseline | `2026-05-21-A-scaffold.md` | — | — |
| B | Contract layer (`shared/proto` + `shared/crypto` + `shared/transport`) | `2026-05-21-B-shared-contract.md` | A | — |
| C | PTY (`server/pty`) | `2026-05-21-C-pty.md` | B | E, F |
| D | Daemon (`server/daemon-core` + `server/daemon-bin`) | `2026-05-21-D-daemon.md` | B, C | E |
| E | Relay (`relay/relay-core` + `relay/relay-bin`) | `2026-05-21-E-relay.md` | B | C, D, F |
| F | Client core (`client/client-core` + `client/client-core-wasm`) | `2026-05-21-F-client-core.md` | B | C, D, E |
| G | Webview (`webview/terminal`) | `2026-05-21-G-webview.md` | F | — |
| H | Tauri apps (`apps/desktop` + `apps/mobile`) | `2026-05-21-H-tauri-apps.md` | F, G | I |
| I | Web app (`apps/web`) | `2026-05-21-I-web-app.md` | F, G | H |
| J | Release pipeline (signing, packaging, release.yml) | `2026-05-21-J-release.md` | H or I (one usable build target is enough to start) | — |

## Dependency Graph

```
                    ┌──────────────────┐
                    │ A. Scaffold + CI │
                    └────────┬─────────┘
                             │
                             ▼
                ┌──────────────────────────┐
                │ B. shared (proto+crypto  │
                │    +transport)            │
                │ ── proto freeze tag ──   │
                └─────┬───────┬───────┬────┘
                      │       │       │
        ┌─────────────┘       │       └─────────────┐
        ▼                     ▼                     ▼
   ┌────────┐           ┌──────────┐           ┌──────────────┐
   │ C. pty │           │ E. relay │           │ F. client-   │
   └───┬────┘           └─────┬────┘           │    core +    │
       │                      │                │    wasm      │
       ▼                      │                └──────┬───────┘
   ┌──────────┐               │                       │
   │ D. daemon│               │                       ▼
   └────┬─────┘               │                 ┌──────────────┐
        │                     │                 │ G. webview/  │
        │                     │                 │    terminal  │
        │                     │                 └──────┬───────┘
        │                     │                        │
        │                     │              ┌─────────┴─────────┐
        │                     │              ▼                   ▼
        │                     │       ┌─────────────┐    ┌──────────────┐
        │                     │       │ H. Tauri    │    │ I. apps/web  │
        │                     │       │  desktop +  │    │              │
        │                     │       │  mobile     │    │              │
        │                     │       └──────┬──────┘    └──────┬───────┘
        │                     │              │                  │
        └─────────────────────┴──────────────┴──────────────────┘
                                      │
                                      ▼
                            ┌────────────────────┐
                            │ J. Release pipeline│
                            └────────────────────┘
```

## Parallel Windows

These windows let multiple sub-plans run concurrently if you have the bandwidth (or are dispatching subagents).

- **After A is validated:** B is the only candidate. B blocks everyone.
- **After B (proto freeze):** C, E, F can run in parallel. They share no state and depend only on the contract layer.
- **After C and B:** D can start. (D doesn't strictly need E, but D's full integration test path needs the relay; the daemon's loopback / direct-LAN tests are usable without it.)
- **After F:** G can start.
- **After G:** H and I can run in parallel.
- **After H or I produces one signable artifact:** J can start in parallel with the other client-side plan.

## Validation Gates

Plans are written upfront in one batch, but execution still has one hard gate:

**B → proto freeze gate.** When Plan B completes its final step, the maintainer tags the contract layer:

```bash
git tag proto-v1.0.0-frozen
git push --tags
```

After this tag, `crates/shared/proto/**` and `crates/shared/crypto/**` changes require an ADR (see below). This protects the four downstream plans (C, D, E, F) from concurrent contract drift.

Plan A itself doesn't gate plan-writing (all plans are written together), but it MUST complete in execution before any other plan starts running tasks — its choices about `just`, CI, and crate paths are what every later plan writes against.

## ADR Mechanism

`docs/superpowers/adr/` holds one Markdown file per non-obvious decision. Filename: `NNNN-kebab-case-title.md` with monotonic 4-digit prefix.

Required ADRs at v1 (write these as part of Plan A or alongside the relevant plan):

| # | Title | Owning plan |
|---|---|---|
| 0001 | Use Rust + Tauri full-stack instead of Electron + Expo | A |
| 0002 | Bytes-plus-anchor scrollback rather than parsed grid | C |
| 0003 | Noise XK over JSON+TLS-only trust model | B |
| 0004 | Self-hosted Rust relay with trait abstraction (CF DO deferred) | E |
| 0005 | Web client is relay-only; no LAN direct from the browser | F / I |
| 0006 | Wasm-friendly `client-core` via four traits, not duplicate TS impl | F |
| 0007 | `minisign` for signing; Apple/Authenticode optional | J |
| 0008 | No auto-update in v1; manual `cli-pocket-daemon update` | J |

Format (template lives in `docs/superpowers/adr/0000-template.md`, created by Plan A):

```markdown
# NNNN. Title

Date: YYYY-MM-DD
Status: Accepted | Superseded by NNNN | Deprecated
Owners: <names>

## Context
Why this decision needed to be made.

## Decision
The decision itself, in one or two paragraphs.

## Consequences
- Positive
- Negative
- Risks accepted
```

## Handoff Notes

After every sub-plan finishes, the implementer writes a short note to `docs/superpowers/handoff/<plan-id>.md` capturing:

- What was actually built (paths, key types, commands).
- Any deviation from the spec or this overview, with rationale.
- Open questions or follow-ups for downstream plans.

Filename matches the plan ID: `A.md`, `B.md`, …, `J.md`.

Downstream plans **must** read the upstream handoff notes before starting work — this is the protection against acting on stale spec assumptions. Each sub-plan's first task is "Read upstream handoff notes."

## Commit Conventions

All tasks in every plan (A-J) follow these rules. Subagents must not deviate.

- **One focused commit per Task.** Each Task's final step is a single `git commit`. Don't squash multiple Tasks together; don't split one Task across commits unless the plan says so.
- **Conventional Commits subject:** `<type>: <subject>` where `<type>` is one of `feat` | `fix` | `chore` | `docs` | `test` | `refactor` | `build` | `ci`. Subject is imperative, lower-case, no trailing period, ≤ 150 chars.
  - `feat:` user-visible behavior change
  - `fix:` user-visible bug fix
  - `chore:` scaffolding, deps, repo housekeeping (no behavior change)
  - `docs:` markdown / comments only
  - `test:` tests only
  - `refactor:` internal restructure, no behavior change
  - `build:` build system / packaging (justfile, Cargo features, Tauri config)
  - `ci:` GitHub Actions, release workflow

Examples (all taken from this plan's Tasks):

```
chore: initialize repo, gitignore, editorconfig, toolchain pins
chore: add Cargo workspace root and shared/proto stub crate
build: add justfile with documented entry points
ci: add GitHub Actions PR gate
```

## Plan Writing & Execution Order

**Plan writing** (this conversation):

1. Overview plan, Plan A, then Plans B–J — all written in one batch up front.
2. Each plan is self-contained: anyone can pick up Plan E and execute it once its upstream dependencies are met.

**Execution** (after all plans are written):

1. Plan A first. The scaffold is what every later plan writes against.
2. Plan B second. Once B completes, the maintainer tags `proto-v1.0.0-frozen`. Plans C–J's task descriptions assume this tag exists.
3. Plans C, E, F can run in parallel after B (see "Parallel Windows" above).
4. Plans D after C, G after F, H/I after G, J after H or I.

## Per-Plan Definition of Done (Quick Reference)

Each sub-plan's full DoD lives in its own file. The one-line summaries below give the overview-level shape so you can tell whether a plan is "really done" without reading 400 lines.

- **A** — `cargo build` and `cargo test` pass on a stub crate per role; CI green on a hello-world commit; `just --list` shows the documented entry points; ADR template and `docs/superpowers/{adr,handoff}/` exist.
- **B** — `proto` round-trips every `Frame` variant under proptest; `crypto` passes Noise XK known-answer vectors plus a SPAKE2 round-trip; `transport` mocks both ends of a WebSocket and exchanges 1 MB of frames.
- **C** — A `pty::Terminal` spawns `cat` / `echo` / a Windows shim, ring buffer enforces capacity, snapshot+delta round-trip via property tests.
- **D** — Daemon binary listens on a configurable port, completes Noise XK with a paired client, opens/closes terminals, honors resume tokens, drops revoked clients on file-watch.
- **E** — Relay binary registers hosts, pairs clients, forwards bytes, enforces 4 capacity limits and a stuck-pair guillotine; `/metrics` and `/health` work.
- **F** — `client-core` builds native and to wasm; reconnect with resume token works against a mock daemon; identity is persisted via the trait abstraction.
- **G** — Vite dual-build (`dist/tauri`, `dist/web`); xterm.js renders snapshot+delta from a mock daemon over both Tauri commands and the wasm binding.
- **H** — Tauri desktop app and Tauri mobile app each launch, list paired hosts, open a terminal, survive a forced reconnect; mobile virtual key bar works.
- **I** — Browser app at a static URL pairs via 6-digit code (relay-only), runs xterm.js, persists identity in IndexedDB, exports/imports identity.
- **J** — Tag push produces signed daemon/relay tarballs, desktop installers, web `.tar.gz`, and mobile artifacts; `minisign -V` succeeds against the published key.

## Where to Find Things

- Spec: `docs/superpowers/specs/2026-05-21-cross-platform-remote-terminal-design.md`
- Plans: `docs/superpowers/plans/2026-05-21-{overview, A, B, C, D, E, F, G, H, I, J}-*.md`
- ADRs: `docs/superpowers/adr/NNNN-*.md`
- Handoff notes: `docs/superpowers/handoff/{A..J}.md`
- Existing repo conventions: `AGENTS.md` (project-level rules — concise, technical, no emoji)

## Execution Posture for This Project

The user's preference (per AGENTS.md and the brainstorming session) is concise, technical, no fluff. Plans should:

- Reference exact file paths.
- Show full code blocks where the engineer would otherwise have to invent the code.
- Avoid backwards-compat hacks unless explicitly requested (this is a greenfield repo at v1).
- Run `npm run check` is **not** a thing here — that's a paseo convention; cli-pocket uses `just check` (defined in Plan A).
- Frequent commits, one focused commit per task per Plan A's commit message style (defined there).

---

This overview itself does not need a "Self-Review" because it has no executable steps. The per-plan self-reviews (in writing-plans skill) cover their own content.
