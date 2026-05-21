# Plan A — Scaffold + CI Baseline

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the empty repository skeleton — Cargo workspace with role groupings, justfile, CI baseline, ADR + handoff infra — such that `just check` and `just test` pass on a hello-world commit and CI is green.

**Architecture:** Cargo workspace under `crates/{shared,server,relay,client}/*`, four placeholder app/webview directories, GitHub Actions CI on x86_64 Linux only at this stage, mdBook for docs, `just` for developer entry points. No application logic in this plan — every crate has a single `lib.rs` exposing one trivial constant or function plus one trivial test.

**Tech Stack:** Rust 1.x stable (pinned), Node LTS, just, cargo-deny, mdBook, GitHub Actions.

**Spec reference:** `docs/superpowers/specs/2026-05-21-cross-platform-remote-terminal-design.md` § Section 1 (workspace layout) and § Section 8 (CI gates, build tooling).

**Overview reference:** `docs/superpowers/plans/2026-05-21-cli-pocket-overview-plan.md`

---

## Definition of Done

- `just check` runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, and exits 0.
- `just test` runs `cargo test --workspace` and exits 0 with at least one passing test per crate.
- `just build-daemon`, `just build-relay`, `just build-webview-tauri`, `just build-webview-web`, `just build-wasm` all complete (the binaries print a hard-coded greeting; webview builds produce a `dist/` index.html).
- `just --list` shows every documented command.
- A push to a branch triggers `.github/workflows/ci.yml` and the workflow goes green.
- `docs/superpowers/adr/0000-template.md` and `docs/superpowers/handoff/.gitkeep` exist.
- `LICENSE` (AGPL-3.0-only full text), `README.md`, `SECURITY.md`, and `.editorconfig` exist at the repo root.
- The handoff note `docs/superpowers/handoff/A.md` is written.

## File Structure

Created in this plan (and only this plan):

```
cli-pocket/
├── .github/
│   └── workflows/
│       └── ci.yml                                  # PR gate (Linux x86_64 only at this stage)
├── .gitignore                                       # Rust / Node / Tauri standard ignores
├── .editorconfig                                    # cross-editor indent / EOL consistency
├── .nvmrc                                           # node LTS pin
├── rust-toolchain.toml                              # rust stable pin
├── LICENSE                                          # AGPL-3.0-only full text
├── README.md                                        # project intro, install/run, links
├── SECURITY.md                                      # vuln reporting policy
├── Cargo.toml                                       # [workspace] root
├── deny.toml                                        # cargo-deny config
├── justfile                                         # developer entry points
├── crates/
│   ├── shared/
│   │   ├── proto/{Cargo.toml, src/lib.rs}
│   │   ├── crypto/{Cargo.toml, src/lib.rs}
│   │   └── transport/{Cargo.toml, src/lib.rs}
│   ├── server/
│   │   ├── pty/{Cargo.toml, src/lib.rs}
│   │   ├── daemon-core/{Cargo.toml, src/lib.rs}
│   │   └── daemon-bin/{Cargo.toml, src/main.rs}
│   ├── relay/
│   │   ├── relay-core/{Cargo.toml, src/lib.rs}
│   │   └── relay-bin/{Cargo.toml, src/main.rs}
│   └── client/
│       ├── client-core/{Cargo.toml, src/lib.rs}
│       └── client-core-wasm/{Cargo.toml, src/lib.rs}
├── apps/
│   ├── desktop/.gitkeep
│   ├── mobile/.gitkeep
│   └── web/.gitkeep
├── webview/
│   └── terminal/
│       ├── package.json
│       ├── vite.config.ts
│       ├── tsconfig.json
│       ├── index.html
│       └── src/main.ts
└── docs/
    └── superpowers/
        ├── adr/
        │   └── 0000-template.md
        └── handoff/
            └── .gitkeep
```

The Cargo workspace `members` glob is `crates/*/*` — the role directories aren't crates themselves.

mdBook setup (`docs/book.toml`) is intentionally deferred to a later plan — there's no real content to render until Plans B–J land.

---

## Task A1 — Initialize git repo and base ignore rules

**Files:**
- Create: `.gitignore`
- Create: `.editorconfig`
- Create: `rust-toolchain.toml`
- Create: `.nvmrc`

- [ ] **Step 1: Verify the repo is not yet a git repo**

Run: `git -C /e/ezra_workspace/cli-pocket status 2>&1 | head -1`
Expected: `fatal: not a git repository (or any of the parent directories): .git` — confirms we're starting fresh.

If it IS already a git repo, skip Step 2 and verify `git log` is empty before continuing.

- [ ] **Step 2: Initialize the repo**

Run: `git -C /e/ezra_workspace/cli-pocket init -b main`
Expected: `Initialized empty Git repository in E:/ezra_workspace/cli-pocket/.git/`

- [ ] **Step 3: Write `.gitignore`**

```gitignore
# Rust
target/
Cargo.lock.bak
**/*.rs.bk

# Node
node_modules/
*.log

# Tauri
apps/*/src-tauri/target/
apps/*/dist/

# Webview
webview/*/dist/
webview/*/.vite/

# Wasm pack
crates/client/client-core-wasm/pkg/

# IDE
.vscode/
.idea/
*.swp
.DS_Store

# OS
Thumbs.db

# Tooling caches
.cache/
*.bak
```

- [ ] **Step 4: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.84.0"
components = ["rustfmt", "clippy"]
targets = ["wasm32-unknown-unknown"]
```

(If Rust 1.84.0 is unavailable when the engineer runs this, bump to the latest stable and update this line accordingly. Document the choice in handoff.)

- [ ] **Step 5: Write `.nvmrc`**

```
20
```

- [ ] **Step 6: Write `.editorconfig`**

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true
indent_style = space
indent_size = 4

[*.{md,yml,yaml,json,toml}]
indent_size = 2

[*.{ts,tsx,js,mjs,cjs,html,css}]
indent_size = 2

[Makefile]
indent_style = tab

[justfile]
indent_style = space
indent_size = 4
```

(Rust source defaults to 4-space indentation per `rustfmt`'s default; the `[*]` rule above matches that. JSON / YAML / TOML use 2 because that's the ecosystem norm.)

- [ ] **Step 7: Stage and commit**

```bash
git add .gitignore .editorconfig rust-toolchain.toml .nvmrc
git commit -m "chore: initialize repo, gitignore, editorconfig, toolchain pins"
```

---

## Task A2 — Cargo workspace root and shared/proto crate

**Files:**
- Create: `Cargo.toml`
- Create: `crates/shared/proto/Cargo.toml`
- Create: `crates/shared/proto/src/lib.rs`

- [ ] **Step 1: Write workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/shared/proto",
    "crates/shared/crypto",
    "crates/shared/transport",
    "crates/server/pty",
    "crates/server/daemon-core",
    "crates/server/daemon-bin",
    "crates/relay/relay-core",
    "crates/relay/relay-bin",
    "crates/client/client-core",
    "crates/client/client-core-wasm",
]

[workspace.package]
edition = "2021"
rust-version = "1.84"
license = "AGPL-3.0-only"
repository = "https://github.com/32r4/cli-pocket"
authors = ["cli-pocket contributors"]

[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"

# Shared dependency versions. Plan B+ will populate this table; per-crate
# Cargo.toml files then declare `<name> = { workspace = true }` instead of
# pinning their own version. Keeping the table empty-but-present in Plan A
# avoids version drift the moment three crates pull in tokio at once.
[workspace.dependencies]
# Populated in Plan B and beyond. Examples (do NOT add yet):
#   tokio       = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "io-util", "net"] }
#   serde       = { version = "1", features = ["derive"] }
#   postcard    = { version = "1", features = ["use-std"] }
#   tracing     = "0.1"

[profile.release]
codegen-units = 1
lto = "thin"
strip = "symbols"
```

(`unsafe_code = "forbid"` will be relaxed in `crates/server/pty` later when wrapping `portable-pty`. That relaxation is a per-crate override and an ADR.)

- [ ] **Step 2: Write `crates/shared/proto/Cargo.toml`**

```toml
[package]
name = "cli-pocket-proto"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Wire protocol contracts for cli-pocket. See docs/superpowers/specs."

[lints]
workspace = true
```

- [ ] **Step 3: Write `crates/shared/proto/src/lib.rs`**

```rust
//! Wire protocol contracts. Real types land in Plan B.

/// Placeholder version used until Plan B lands the real protocol.
pub const SCAFFOLD_VERSION: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_version_is_zero() {
        assert_eq!(SCAFFOLD_VERSION, 0);
    }
}
```

- [ ] **Step 4: Verify the workspace resolves**

Run: `cargo check -p cli-pocket-proto`
Expected: a successful check; no errors.

If `cargo check` complains that other workspace members are missing — this is fine: we'll create them in the next tasks. But the immediate failure should mention only the missing members, not a syntax error. If it complains about something else, fix it before continuing.

(Workaround if Cargo refuses to build with members that don't yet exist: comment out everything except `crates/shared/proto` in the workspace `members = [...]`, then re-add each member as you create it in Tasks A3–A10.)

- [ ] **Step 5: Run the test**

Run: `cargo test -p cli-pocket-proto`
Expected: `test scaffold_version_is_zero ... ok`, 1 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/shared/proto
git commit -m "chore: add Cargo workspace root and shared/proto stub crate"
```

---

## Task A3 — `shared/crypto` and `shared/transport` stubs

**Files:**
- Create: `crates/shared/crypto/Cargo.toml`
- Create: `crates/shared/crypto/src/lib.rs`
- Create: `crates/shared/transport/Cargo.toml`
- Create: `crates/shared/transport/src/lib.rs`

- [ ] **Step 1: Write `crates/shared/crypto/Cargo.toml`**

```toml
[package]
name = "cli-pocket-crypto"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Noise XK + SPAKE2 wrappers for cli-pocket. Real impl lands in Plan B."

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/shared/crypto/src/lib.rs`**

```rust
//! Cryptographic primitives. Real types land in Plan B.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_compiles() {
        // Deliberately trivial. Plan B replaces this whole crate.
    }
}
```

- [ ] **Step 3: Write `crates/shared/transport/Cargo.toml`**

```toml
[package]
name = "cli-pocket-transport"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "WebSocket transport abstraction for cli-pocket. Real impl lands in Plan B."

[lints]
workspace = true
```

- [ ] **Step 4: Write `crates/shared/transport/src/lib.rs`**

```rust
//! WebSocket transport abstraction. Real types land in Plan B.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_compiles() {}
}
```

- [ ] **Step 5: Verify**

Run: `cargo test -p cli-pocket-crypto -p cli-pocket-transport`
Expected: 1 passed in each crate.

- [ ] **Step 6: Commit**

```bash
git add crates/shared/crypto crates/shared/transport
git commit -m "chore: add shared/crypto and shared/transport stubs"
```

---

## Task A4 — Server crates: `pty`, `daemon-core`, `daemon-bin`

**Files:**
- Create: `crates/server/pty/Cargo.toml`
- Create: `crates/server/pty/src/lib.rs`
- Create: `crates/server/daemon-core/Cargo.toml`
- Create: `crates/server/daemon-core/src/lib.rs`
- Create: `crates/server/daemon-bin/Cargo.toml`
- Create: `crates/server/daemon-bin/src/main.rs`

- [ ] **Step 1: Write `crates/server/pty/Cargo.toml`**

```toml
[package]
name = "cli-pocket-pty"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "PTY wrapper. Real impl lands in Plan C."

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/server/pty/src/lib.rs`**

```rust
//! PTY wrapper around portable-pty. Real types land in Plan C.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_compiles() {}
}
```

- [ ] **Step 3: Write `crates/server/daemon-core/Cargo.toml`**

```toml
[package]
name = "cli-pocket-daemon-core"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Daemon library. Real impl lands in Plan D."

[dependencies]
cli-pocket-proto      = { path = "../../shared/proto" }
cli-pocket-crypto     = { path = "../../shared/crypto" }
cli-pocket-transport  = { path = "../../shared/transport" }
cli-pocket-pty        = { path = "../pty" }

[lints]
workspace = true
```

- [ ] **Step 4: Write `crates/server/daemon-core/src/lib.rs`**

```rust
//! Daemon orchestration. Real types land in Plan D.

pub fn version_banner() -> String {
    format!(
        "cli-pocket-daemon (scaffold proto v{})",
        cli_pocket_proto::SCAFFOLD_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_mentions_proto_version() {
        assert!(version_banner().contains("proto v0"));
    }
}
```

- [ ] **Step 5: Write `crates/server/daemon-bin/Cargo.toml`**

```toml
[package]
name = "cli-pocket-daemon"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "cli-pocket daemon binary. Real impl lands in Plan D."

[[bin]]
name = "cli-pocket-daemon"
path = "src/main.rs"

[dependencies]
cli-pocket-daemon-core = { path = "../daemon-core" }

[lints]
workspace = true
```

- [ ] **Step 6: Write `crates/server/daemon-bin/src/main.rs`**

```rust
fn main() {
    println!("{}", cli_pocket_daemon_core::version_banner());
}
```

- [ ] **Step 7: Verify**

Run: `cargo test -p cli-pocket-pty -p cli-pocket-daemon-core`
Expected: 1 passing test in each.

Run: `cargo run -p cli-pocket-daemon`
Expected: `cli-pocket-daemon (scaffold proto v0)`

- [ ] **Step 8: Commit**

```bash
git add crates/server
git commit -m "chore: add server/{pty,daemon-core,daemon-bin} stubs"
```

---

## Task A5 — Relay crates: `relay-core`, `relay-bin`

**Files:**
- Create: `crates/relay/relay-core/Cargo.toml`
- Create: `crates/relay/relay-core/src/lib.rs`
- Create: `crates/relay/relay-bin/Cargo.toml`
- Create: `crates/relay/relay-bin/src/main.rs`

- [ ] **Step 1: Write `crates/relay/relay-core/Cargo.toml`**

```toml
[package]
name = "cli-pocket-relay-core"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Relay library. Real impl lands in Plan E."

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/relay/relay-core/src/lib.rs`**

```rust
//! Relay forwarding logic. Real types land in Plan E.

pub fn version_banner() -> &'static str {
    "cli-pocket-relay (scaffold)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_is_relay() {
        assert!(version_banner().contains("relay"));
    }
}
```

- [ ] **Step 3: Write `crates/relay/relay-bin/Cargo.toml`**

```toml
[package]
name = "cli-pocket-relay"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "cli-pocket relay binary. Real impl lands in Plan E."

[[bin]]
name = "cli-pocket-relay"
path = "src/main.rs"

[dependencies]
cli-pocket-relay-core = { path = "../relay-core" }

[lints]
workspace = true
```

- [ ] **Step 4: Write `crates/relay/relay-bin/src/main.rs`**

```rust
fn main() {
    println!("{}", cli_pocket_relay_core::version_banner());
}
```

- [ ] **Step 5: Verify**

Run: `cargo test -p cli-pocket-relay-core`
Expected: 1 passed.

Run: `cargo run -p cli-pocket-relay`
Expected: `cli-pocket-relay (scaffold)`

- [ ] **Step 6: Commit**

```bash
git add crates/relay
git commit -m "chore: add relay/{relay-core,relay-bin} stubs"
```

---

## Task A6 — Client crates: `client-core` and `client-core-wasm`

**Files:**
- Create: `crates/client/client-core/Cargo.toml`
- Create: `crates/client/client-core/src/lib.rs`
- Create: `crates/client/client-core-wasm/Cargo.toml`
- Create: `crates/client/client-core-wasm/src/lib.rs`

- [ ] **Step 1: Write `crates/client/client-core/Cargo.toml`**

```toml
[package]
name = "cli-pocket-client-core"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Client state machine. Real impl lands in Plan F."

[dependencies]
cli-pocket-proto      = { path = "../../shared/proto" }
cli-pocket-crypto     = { path = "../../shared/crypto" }
cli-pocket-transport  = { path = "../../shared/transport" }

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/client/client-core/src/lib.rs`**

```rust
//! Client state machine. Real types land in Plan F.
//!
//! Must remain wasm-friendly: no tokio multi-thread, no std::net direct,
//! no direct std::time::Instant outside trait impls. Plan F enforces this.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_compiles() {}
}
```

- [ ] **Step 3: Write `crates/client/client-core-wasm/Cargo.toml`**

```toml
[package]
name = "cli-pocket-client-core-wasm"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "wasm-bindgen wrapper over client-core. Real impl lands in Plan F."

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cli-pocket-client-core = { path = "../client-core" }

[lints]
workspace = true
```

- [ ] **Step 4: Write `crates/client/client-core-wasm/src/lib.rs`**

```rust
//! wasm-bindgen surface. Real types land in Plan F.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_compiles() {}
}
```

- [ ] **Step 5: Verify native compile**

Run: `cargo test -p cli-pocket-client-core -p cli-pocket-client-core-wasm`
Expected: 1 passing test in each.

- [ ] **Step 6: Verify wasm target compiles**

Run: `cargo build --target wasm32-unknown-unknown -p cli-pocket-client-core-wasm`
Expected: success.

If `wasm32-unknown-unknown` target isn't installed: `rustup target add wasm32-unknown-unknown`. Document this in the handoff so the next contributor knows.

- [ ] **Step 7: Commit**

```bash
git add crates/client
git commit -m "chore: add client/{client-core,client-core-wasm} stubs"
```

---

## Task A7 — Apps and webview placeholders

**Files:**
- Create: `apps/desktop/.gitkeep`
- Create: `apps/mobile/.gitkeep`
- Create: `apps/web/.gitkeep`
- Create: `webview/terminal/package.json`
- Create: `webview/terminal/tsconfig.json`
- Create: `webview/terminal/vite.config.ts`
- Create: `webview/terminal/index.html`
- Create: `webview/terminal/src/main.ts`

- [ ] **Step 1: Create app placeholders**

```bash
mkdir -p /e/ezra_workspace/cli-pocket/apps/desktop
mkdir -p /e/ezra_workspace/cli-pocket/apps/mobile
mkdir -p /e/ezra_workspace/cli-pocket/apps/web
touch /e/ezra_workspace/cli-pocket/apps/desktop/.gitkeep
touch /e/ezra_workspace/cli-pocket/apps/mobile/.gitkeep
touch /e/ezra_workspace/cli-pocket/apps/web/.gitkeep
```

- [ ] **Step 2: Write `webview/terminal/package.json`**

```json
{
  "name": "@cli-pocket/webview-terminal",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "build:tauri": "VITE_CLIENT_KIND=tauri vite build --outDir dist/tauri",
    "build:web":   "VITE_CLIENT_KIND=web   vite build --outDir dist/web",
    "lint":        "tsc --noEmit",
    "test":        "echo 'no webview tests yet (Plan G adds vitest)' && exit 0"
  },
  "devDependencies": {
    "typescript": "^5.6.0",
    "vite": "^5.4.0"
  }
}
```

(Note: cross-platform env-var setting via `cross-env` is intentionally not added here — Plan G will introduce it when the build matters. For Plan A we just need Vite to produce a `dist/` index.)

- [ ] **Step 3: Write `webview/terminal/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noImplicitOverride": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "resolveJsonModule": true,
    "lib": ["ES2022", "DOM"]
  },
  "include": ["src", "vite.config.ts"]
}
```

- [ ] **Step 4: Write `webview/terminal/vite.config.ts`**

```ts
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    target: "es2022",
    sourcemap: true,
  },
  define: {
    __CLIENT_KIND__: JSON.stringify(process.env.VITE_CLIENT_KIND ?? "tauri"),
  },
});
```

- [ ] **Step 5: Write `webview/terminal/index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width,initial-scale=1" />
    <title>cli-pocket scaffold</title>
  </head>
  <body>
    <main id="root">cli-pocket webview scaffold</main>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 6: Write `webview/terminal/src/main.ts`**

```ts
declare const __CLIENT_KIND__: "tauri" | "web";

const root = document.getElementById("root");
if (root) {
  root.textContent = `cli-pocket webview (kind=${__CLIENT_KIND__})`;
}
```

- [ ] **Step 7: Install and build**

Run: `cd webview/terminal && npm install && npm run build:tauri`
Expected: a `dist/tauri/index.html` is produced. The terminal output mentions Vite has built successfully.

Run: `cd webview/terminal && npm run build:web`
Expected: a `dist/web/index.html` is produced.

If `npm install` is glacial on Windows, that is normal — it does not block this plan. Do not change registries.

- [ ] **Step 8: Commit**

```bash
git add apps webview
git commit -m "chore: add app placeholders and webview/terminal Vite scaffold"
```

---

## Task A8 — `justfile` with documented entry points

**Files:**
- Create: `justfile`

- [ ] **Step 1: Verify `just` is installed**

Run: `just --version`
Expected: `just 1.x.x` or higher.

If absent: `cargo install just` or `winget install just` (Windows) / `brew install just` (macOS).

- [ ] **Step 2: Write `justfile`**

```just
# cli-pocket developer commands. Run `just --list` for a summary.

default:
    @just --list

# ---- workspace-wide gates ----

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    cd webview/terminal && npm run lint

test:
    cargo test --workspace
    cd webview/terminal && npm test

# ---- one-time setup ----

# Installs the tauri-cli used by `cargo tauri` in apps/desktop and apps/mobile,
# plus wasm-pack used by Plans F and I. Idempotent.
setup:
    cargo install tauri-cli --version "^2.1" --locked
    cargo install wasm-pack --locked

# ---- per-target builds ----

build-daemon:
    cargo build --release -p cli-pocket-daemon

build-relay:
    cargo build --release -p cli-pocket-relay

build-wasm:
    wasm-pack build crates/client/client-core-wasm --target web --release

build-webview-tauri:
    cd webview/terminal && npm run build:tauri

build-webview-web:
    cd webview/terminal && npm run build:web

# Built in Plan H — recipes shown here so `just --list` is the single index.
build-desktop:
    cd webview/terminal && npm run build:tauri
    cd apps/desktop && cargo tauri build

build-mobile-android:
    cd webview/terminal && npm run build:tauri
    cd apps/mobile && cargo tauri android build --apk --aab

build-mobile-ios:
    cd webview/terminal && npm run build:tauri
    cd apps/mobile && cargo tauri ios build

# Built in Plan I — needs `just build-wasm` first.
build-web:
    just build-wasm
    cd apps/web && npm run build

# ---- dev workflows ----

dev-daemon:
    cargo run -p cli-pocket-daemon

dev-relay:
    cargo run -p cli-pocket-relay

dev-desktop:
    cd apps/desktop && cargo tauri dev

dev-mobile-android:
    cd apps/mobile && cargo tauri android dev

dev-mobile-ios:
    cd apps/mobile && cargo tauri ios dev

dev-web:
    cd apps/web && npm run dev

# ---- maintenance ----

fmt:
    cargo fmt
    cd webview/terminal && npx tsc --noEmit

clean:
    cargo clean
    rm -rf webview/terminal/dist webview/terminal/node_modules
    rm -rf apps/web/dist apps/web/node_modules

# ---- release ----

# Builds every artifact this project produces. Slow.
dist:
    just build-daemon
    just build-relay
    just build-wasm
    just build-webview-tauri
    just build-webview-web
```

- [ ] **Step 3: Run `just --list`**

Run: `just --list`
Expected: every recipe (`check`, `test`, `setup`, `build-daemon`, `build-relay`, `build-wasm`, `build-webview-tauri`, `build-webview-web`, `build-desktop`, `build-mobile-android`, `build-mobile-ios`, `build-web`, `dev-daemon`, `dev-relay`, `dev-desktop`, `dev-mobile-android`, `dev-mobile-ios`, `dev-web`, `fmt`, `clean`, `dist`) is shown.

- [ ] **Step 4: Commit**

```bash
git add justfile
git commit -m "chore: add justfile with documented developer entry points"
```

---

## Task A9 — `cargo-deny` configuration

**Files:**
- Create: `deny.toml`

- [ ] **Step 1: Verify `cargo-deny` is installed**

Run: `cargo deny --version`
Expected: a version string.

If absent: `cargo install cargo-deny --locked`.

- [ ] **Step 2: Write `deny.toml`**

```toml
[graph]
all-features = true

[advisories]
db-path = "~/.cargo/advisory-db"
db-urls = ["https://github.com/rustsec/advisory-db"]
yanked = "warn"
ignore = []

[licenses]
unlicensed = "deny"
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-DFS-2016",
    "Unicode-3.0",
    "Zlib",
    "MPL-2.0",
    "AGPL-3.0",
    "AGPL-3.0-only",
    "0BSD",
    "CC0-1.0",
]
copyleft = "warn"
default = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- [ ] **Step 3: Run `cargo deny check`**

Run: `cargo deny check`
Expected: warnings are tolerable; no `error:` lines.

If there are errors, the most likely cause is a license not in the allow list above. Add the license name (verify it's actually OSI-approved before adding).

- [ ] **Step 4: Commit**

```bash
git add deny.toml
git commit -m "chore: add cargo-deny configuration"
```

---

## Task A10 — Verify `just check` and `just test` pass

- [ ] **Step 1: Run `just check`**

Run: `just check`
Expected: each step passes; exit code 0.

If `cargo fmt --check` fails, run `cargo fmt` and re-stage. Commit the formatting separately if needed.

If `cargo clippy` fails, fix the warnings. The scaffold code should not produce any.

- [ ] **Step 2: Run `just test`**

Run: `just test`
Expected: each crate reports 1 passing test (or 0 for `crypto`/`transport`/`pty`/`client-core`/`client-core-wasm`/`proto-as-of-step` — count what you actually have). The webview test step prints its placeholder message and exits 0.

- [ ] **Step 3: Commit any fmt-only changes if `cargo fmt` modified files**

```bash
git add -u
git commit -m "chore: cargo fmt"
```

(Skip this step if there were no formatting changes.)

---

## Task A11 — GitHub Actions CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write `.github/workflows/ci.yml`**

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: "-D warnings"

jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Read rust-toolchain
        run: cat rust-toolchain.toml

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
          targets: wasm32-unknown-unknown

      - uses: Swatinem/rust-cache@v2

      - name: Install cargo-deny
        run: cargo install --locked cargo-deny

      - run: cargo fmt --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: cargo build --target wasm32-unknown-unknown -p cli-pocket-client-core-wasm
      - run: cargo deny check

  webview:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version-file: .nvmrc
          cache: npm
          cache-dependency-path: webview/terminal/package-lock.json
      - run: npm ci
        working-directory: webview/terminal
      - run: npm run lint
        working-directory: webview/terminal
      - run: npm run build:tauri
        working-directory: webview/terminal
      - run: npm run build:web
        working-directory: webview/terminal
```

- [ ] **Step 2: Generate `package-lock.json` if missing**

Run: `cd webview/terminal && npm install --package-lock-only`
Expected: `webview/terminal/package-lock.json` exists.

- [ ] **Step 3: Commit**

```bash
git add .github webview/terminal/package-lock.json
git commit -m "ci: add Linux x86_64 PR gate workflow"
```

- [ ] **Step 4: Push to a branch and verify CI**

The user pushes `main` to a remote of their choice and watches the workflow. CI should be green.

If CI fails for environmental reasons (cache miss, ephemeral network), re-run. If it fails on a real issue, fix it before declaring Plan A done.

---

## Task A11.5 — Project surface files: LICENSE, README, SECURITY

**Files:**
- Create: `LICENSE`
- Create: `README.md`
- Create: `SECURITY.md`

- [ ] **Step 1: Write `LICENSE`**

The workspace `Cargo.toml` declares `license = "AGPL-3.0-only"`, so the repo MUST ship the AGPL-3.0 full text.

Fetch the canonical text from the FSF and save it verbatim to `LICENSE`:

```bash
curl -fsSL https://www.gnu.org/licenses/agpl-3.0.txt -o LICENSE
wc -l LICENSE   # expect ~660 lines
head -2 LICENSE # expect: "                    GNU AFFERO GENERAL PUBLIC LICENSE"
```

If the network is unavailable, copy the text from a known-good source (e.g. another AGPL project on the local machine, or `https://spdx.org/licenses/AGPL-3.0-only.html`). The file MUST be the full license, not a stub.

Do NOT modify the license text. Do NOT add a copyright header above it in this file — the per-source-file header is a separate concern (Plan B+ may add `// SPDX-License-Identifier: AGPL-3.0-only` to source files).

- [ ] **Step 2: Write `README.md`**

```markdown
# cli-pocket

> Cross-platform remote terminal. Self-hosted, end-to-end encrypted, no SaaS.

cli-pocket is an OSS remote terminal: run a daemon on the machine you want to
reach, connect from desktop / mobile / web. Pairing is end-to-end via Noise XK
or a 6-digit SPAKE2 code. The optional self-hosted relay only forwards
ciphertext.

**Status:** pre-alpha. The scaffold is up; the protocol and clients are being
implemented per the plans under `docs/superpowers/plans/`.

## Quick start (developer)

```bash
just --list           # see all entry points
just check            # fmt + clippy + cargo-deny + webview lint
just test             # cargo test --workspace
just build-daemon     # release build of the daemon
```

Requires Rust (pinned in `rust-toolchain.toml`), Node (pinned in `.nvmrc`),
`just`, and `cargo-deny`. See `docs/superpowers/specs/` for the full design.

## Architecture

| Crate group | What it does |
|---|---|
| `crates/shared/{proto,crypto,transport}` | Wire protocol, Noise XK + SPAKE2, WebSocket transport |
| `crates/server/{pty,daemon-core,daemon-bin}` | The host-side daemon |
| `crates/relay/{relay-core,relay-bin}` | Optional self-hosted relay |
| `crates/client/{client-core,client-core-wasm}` | Client state machine, native + wasm |
| `apps/{desktop,mobile,web}` | Tauri 2 desktop, Tauri Mobile, browser app |
| `webview/terminal` | Shared xterm.js view loaded by every app |

## Security

See [SECURITY.md](./SECURITY.md) for vulnerability reporting.

## License

[AGPL-3.0-only](./LICENSE).
```

- [ ] **Step 3: Write `SECURITY.md`**

```markdown
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

- Vulnerabilities in upstream dependencies — please report those upstream
  (we'll cut a patch release once an upstream fix is available).
- Social engineering of repository maintainers.
- Denial of service against a relay you do not own.

## Hall of Fame

Researchers who report valid issues will be credited in release notes if they
wish.
```

(The GitHub URL `32r4/cli-pocket` matches `repository` in `Cargo.toml`. If the actual repo lives elsewhere, update both files together.)

- [ ] **Step 4: Verify files render**

Run: `wc -l LICENSE README.md SECURITY.md`
Expected: LICENSE ~660 lines, README.md and SECURITY.md each non-empty.

Run: `just check`
Expected: still passes — these are docs, not code, but a stray Markdown file shouldn't break anything.

- [ ] **Step 5: Commit**

```bash
git add LICENSE README.md SECURITY.md
git commit -m "docs: add LICENSE (AGPL-3.0), README, and SECURITY policy"
```

---

## Task A12 — ADR + handoff infrastructure

**Files:**
- Create: `docs/superpowers/adr/0000-template.md`
- Create: `docs/superpowers/handoff/.gitkeep`

- [ ] **Step 1: Create directories**

```bash
mkdir -p /e/ezra_workspace/cli-pocket/docs/superpowers/adr
mkdir -p /e/ezra_workspace/cli-pocket/docs/superpowers/handoff
touch /e/ezra_workspace/cli-pocket/docs/superpowers/handoff/.gitkeep
```

- [ ] **Step 2: Write `docs/superpowers/adr/0000-template.md`**

```markdown
# NNNN. Title

Date: YYYY-MM-DD
Status: Accepted | Superseded by NNNN | Deprecated
Owners: <names>

## Context

Why this decision needed to be made. What forces are at play, what
constraints, what alternatives were considered.

## Decision

The decision itself, in one or two paragraphs. Plain language.

## Consequences

- Positive consequences and what we now get to do.
- Negative consequences and what we now can't do (or have to do).
- Risks accepted with this decision.
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/adr docs/superpowers/handoff
git commit -m "docs: add ADR template and handoff directory"
```

---

## Task A13 — Write the Plan A handoff note

**Files:**
- Create: `docs/superpowers/handoff/A.md`

- [ ] **Step 1: Capture what was actually built**

Run: `cargo metadata --format-version 1 --no-deps | python -c 'import json, sys; m = json.load(sys.stdin); print("\n".join(p["name"] for p in m["packages"]))' 2>/dev/null || cargo tree --workspace --depth 0 | head -40`
Expected: a list of the 10 workspace crates. Capture this for the handoff note.

(If neither command is available — Windows without Python — list the directories under `crates/*/*` manually.)

- [ ] **Step 2: Write `docs/superpowers/handoff/A.md`**

```markdown
# Handoff — Plan A (Scaffold + CI baseline)

Date completed: YYYY-MM-DD
Implementer: <name>

## What was built

Cargo workspace at the repo root with members:

- `crates/shared/{proto,crypto,transport}`
- `crates/server/{pty,daemon-core,daemon-bin}`
- `crates/relay/{relay-core,relay-bin}`
- `crates/client/{client-core,client-core-wasm}`

Each crate has a `Cargo.toml` and `src/{lib,main}.rs` with at least one
trivial test that passes.

App placeholders: `apps/{desktop,mobile,web}/.gitkeep`.

Webview scaffold: `webview/terminal/` produces `dist/{tauri,web}/index.html`
when invoked via `npm run build:tauri` or `npm run build:web`.

Tooling:

- `rust-toolchain.toml` pins Rust to `1.84.0` (or whatever was current).
- `.nvmrc` pins Node to LTS 20.
- `.editorconfig` standardizes indent / EOL across editors.
- `justfile` defines `check`, `test`, `setup`, `build-{daemon,relay,wasm}`,
  `build-webview-{tauri,web}`, `build-{desktop,web,mobile-android,mobile-ios}`,
  `dev-{daemon,relay,desktop,web,mobile-android,mobile-ios}`, `fmt`, `clean`, `dist`.
- `deny.toml` configures `cargo-deny` with the AGPL-3.0 + standard OSS
  license allow list.
- Workspace `Cargo.toml` reserves an empty `[workspace.dependencies]` table
  for Plan B+ to populate.

Project surface: `LICENSE` (AGPL-3.0-only full text), `README.md` (intro +
quickstart + crate map), `SECURITY.md` (private vuln reporting via GitHub
Security Advisories).

CI: `.github/workflows/ci.yml` runs on `ubuntu-latest`, gates fmt, clippy,
test (workspace), wasm build, cargo-deny, and the webview build (tauri + web).
Other OSes are deferred to Plan H/I per the spec's PR-latency design choice.

## Deviations from spec

<list any. If none: "None.">

## Open questions / follow-ups

- Plan B should add real dependencies (`postcard`, `snow`, `spake2`,
  `tokio-tungstenite`, `serde`, `proptest`) to `shared/{proto,crypto,
  transport}` and replace the placeholder modules.
- Plan B is when `npm test` for the webview becomes meaningful — Plan G
  fully replaces the placeholder script.
- The `[lints]` section in the workspace `Cargo.toml` forbids `unsafe_code`.
  The `pty` crate (Plan C) will need a per-crate `unsafe_code = "allow"`
  override when wrapping `portable-pty`. ADR 0002 covers this.
- macOS / Windows CI runners are deferred until end-to-end tests in Plan
  D and H need them.

## Validation

- `just check` — passes locally.
- `just test` — passes locally; <N> tests across the workspace.
- `cargo build --target wasm32-unknown-unknown -p cli-pocket-client-core-wasm` — passes.
- CI workflow at <URL> — green.
```

- [ ] **Step 3: Fill in the placeholders, commit**

Replace `YYYY-MM-DD`, `<name>`, `<list any>`, `<N>`, `<URL>` with real values.

```bash
git add docs/superpowers/handoff/A.md
git commit -m "docs: add Plan A handoff note"
```

---

## Self-Review Checklist (run after Task A13)

1. **Spec coverage:**
   - § Section 1 workspace layout: Tasks A2–A7 ✓
   - § Section 8 build tooling (just, rust-toolchain, .nvmrc, .editorconfig): Tasks A1, A8 ✓
   - § Section 8 CI gates: Task A11 ✓
   - OSS surface (LICENSE, README, SECURITY): Task A11.5 ✓
   - § Overview ADR + handoff infra: Tasks A12, A13 ✓
   - Wasm target build: A6, A11 ✓

2. **Placeholder scan:** No "TODO", "TBD", "fill in", or "implement later" left in the plan that isn't an explicit hand-off to a later plan with a section reference.

3. **Type consistency:** No types are introduced in this plan; the only cross-crate reference is `cli_pocket_proto::SCAFFOLD_VERSION` used in `daemon-core`, defined in `proto/src/lib.rs`. ✓

4. **Internal consistency:**
   - All `Cargo.toml` paths align with the workspace `members` list. ✓
   - All `package.json` script names align with the justfile and CI workflow. ✓
   - The `[workspace.lints.rust] unsafe_code = "forbid"` decision is flagged in the handoff note as needing a per-crate override in Plan C. ✓

If anything in the above review fails when the engineer actually runs the steps, pause and update the plan before continuing.

---

## Execution Posture

When ready to execute, the user will pick:

1. **Subagent-Driven** (recommended) — fresh subagent per task, review between tasks.
2. **Inline Execution** — batch execution with checkpoints.

Either way, after Task A13 the user runs `just check && just test` once more, pushes to a branch, watches CI go green, and only then signals "Plan A approved" — at which point the writing-plans skill is re-invoked to write Plans B–J in one batch.
