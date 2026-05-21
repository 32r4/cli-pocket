# Handoff - Plan A (Scaffold + CI baseline)

Date completed: 2026-05-21
Implementer: Codex

## What was built

Cargo workspace at the repo root with these members, captured from
`cargo metadata --format-version 1 --no-deps`:

- `cli-pocket-proto` at `crates/shared/proto`
- `cli-pocket-crypto` at `crates/shared/crypto`
- `cli-pocket-transport` at `crates/shared/transport`
- `cli-pocket-pty` at `crates/server/pty`
- `cli-pocket-daemon-core` at `crates/server/daemon-core`
- `cli-pocket-daemon` at `crates/server/daemon-bin`
- `cli-pocket-relay-core` at `crates/relay/relay-core`
- `cli-pocket-relay` at `crates/relay/relay-bin`
- `cli-pocket-client-core` at `crates/client/client-core`
- `cli-pocket-client-core-wasm` at `crates/client/client-core-wasm`

Each crate has a `Cargo.toml` and `src/{lib,main}.rs`. The workspace has 8
passing Rust unit tests; binary crates and doctest targets currently have 0
tests.

App placeholders: `apps/{desktop,mobile,web}/.gitkeep`.

Webview scaffold: `webview/terminal/` has Vite, TypeScript, and scripts for
`build:tauri`, `build:web`, `lint`, and placeholder `test`.

Tooling:

- `rust-toolchain.toml` pins Rust to `1.84.0`.
- `.nvmrc` pins Node to LTS 20.
- `.editorconfig` standardizes indent / EOL across editors.
- `justfile` defines `check`, `test`, `setup`, `build-{daemon,relay,wasm}`,
  `build-webview-{tauri,web}`, `build-{desktop,web,mobile-android,mobile-ios}`,
  `dev-{daemon,relay,desktop,web,mobile-android,mobile-ios}`, `fmt`, `clean`,
  and `dist`.
- `deny.toml` configures `cargo-deny` with the AGPL-3.0 + standard OSS
  license allow list.
- Workspace `Cargo.toml` reserves an empty `[workspace.dependencies]` table
  for Plan B+ to populate.
- Crate metadata reports `publish = []`, reflecting `publish = false` for the
  scaffold crates.

Project surface: `LICENSE` (AGPL-3.0-only full text), `README.md` (intro +
quickstart + crate map), `SECURITY.md` (private vulnerability reporting via
GitHub Security Advisories).

CI: `.github/workflows/ci.yml` runs on `ubuntu-latest`, gates fmt, clippy,
test (workspace), wasm build, cargo-deny, and webview lint/build. Other OSes
are deferred to Plan H/I per the spec's PR-latency design choice.

## Deviations from spec

- `cargo-deny` was adjusted for cargo-deny 0.19 schema compatibility: the
  unsupported license fields from the original draft are absent, and
  `allow-wildcard-paths = true` is present so local path-only workspace crates
  can keep wildcard path requirements.
- `cargo-deny` emits unmatched-license warnings for allow-list entries not
  currently present in the dependency graph. They are warnings only; the gate
  exits 0.
- `publish = false` was added to scaffold crate manifests to prevent accidental
  publication of placeholder crates.
- The `justfile` uses Windows shell/platform helpers and `npm --prefix` so the
  recipes work locally on Windows and in Unix CI.
- Direct `npm run build:tauri` and `npm run build:web` scripts remain POSIX
  environment-assignment style. The cross-platform path is through the `just`
  recipes.
- CI was not pushed or observed from this local orchestration.

## Open questions / follow-ups

- Plan B should add real dependencies (`postcard`, `snow`, `spake2`,
  `tokio-tungstenite`, `serde`, `proptest`) to `shared/{proto,crypto,
  transport}` and replace the placeholder modules.
- Plan G is when `npm test` for the webview becomes meaningful and the
  placeholder script is replaced.
- The `[lints]` section in the workspace `Cargo.toml` forbids `unsafe_code`.
  The `pty` crate (Plan C) will need a per-crate `unsafe_code = "allow"`
  override when wrapping `portable-pty`. The unsafe override decision is
  deferred to Plan C / future ADR 0002.
- macOS / Windows CI runners are deferred until end-to-end tests in Plan D and
  H need them.

## Validation

- `cargo metadata --format-version 1 --no-deps` - passed; captured 10 workspace
  packages listed above.
- `npm --prefix webview/terminal install` - passed; required locally before
  `just check` because `node_modules` was not present.
- `just check` - passed locally. Rust fmt and clippy passed; `cargo deny check`
  passed with unmatched-license warnings; webview `tsc --noEmit` passed.
- `just test` - passed locally; 8 Rust unit tests passed, doctests had 0 tests,
  and the webview placeholder test exited 0.
- `cargo build --target wasm32-unknown-unknown -p cli-pocket-client-core-wasm`
  - passed locally.
- CI workflow - Not run locally; no push performed in this orchestration.
