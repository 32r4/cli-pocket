# Development Rules

## Conversational Style

- Keep answers short, concise, direct and technical.
- No emojis in commits, issues, PR comments, or code.
- No fluff or cheerful filler text.
- When the user asks a question, answer it first briefly, then continue with implementation if the intent clearly includes a code change.

## Code Quality

- Read files in full before broad changes, before editing files you have not already inspected, and when investigating or auditing.
- In Rust, avoid `unwrap()` / `expect()` outside tests and `main` startup; return `Result` instead.
- In TypeScript, no `any` types unless absolutely necessary.
- Avoid trivial single-use helper functions when inlining is clearer; keep them when they improve naming or readability.
- Prefer existing library types over guessing; check `Cargo.toml` / `node_modules` when needed.
- In TypeScript application source, prefer top-level static imports. Use dynamic imports only when runtime behavior requires them or when explicitly requested.
- Do not work around dependency type issues by removing functionality. Prefer upgrading the dependency, but ask first if the upgrade may have broader impact.
- Always ask before removing functionality or code that appears to be intentional.
- For internal refactors and local features, do not preserve backward compatibility unless requested. For public APIs or shared contracts (wire protocol, plugin API, CLI flags), ask first.
- Do not make incidental refactors or formatting changes outside the task scope.

## Commands

- After code changes (not documentation changes), run `just check` from the repository root and inspect the full output.
- `just check` is the required pre-commit gate. Fix all newly introduced errors and warnings before committing. If pre-existing issues exist, call them out explicitly before committing.
- `just check` does not run tests.
- Do not run these unless the user explicitly asks: `just dev-*`, `just build-*`, `just dist`, `just test`.
- Do not run full tests. Run a specific Rust test: `cargo test -p <crate> <test_name>`. Run a specific webview test: `cd webview/terminal && npm test -- <path>`.
- Run tests from the workspace root for Rust (`cargo test -p <crate>`); only `cd` into `webview/terminal/` or `apps/web/` for their package-local commands.

## **CRITICAL** Git Rules for Parallel Agents **CRITICAL**

Multiple agents may work on different files in the same worktree simultaneously. You MUST follow these rules:

### Committing

- Never revert or overwrite changes you did not make.
- Only commit files you changed in this session.
- Stage files explicitly; never use `git add .` or `git add -A`.
- Before committing, run `git status` and verify only your files are staged.
- Include `fixes #<number>` or `closes #<number>` when there is a related issue.

### Forbidden Git Operations

These commands can destroy other agents' work:

- `git reset --hard` - destroys uncommitted changes.
- `git checkout .` - destroys uncommitted changes.
- `git checkout -- <path>` - can overwrite another agent's uncommitted changes.
- `git clean -fd` - deletes untracked files.
- `git restore .` - destroys uncommitted changes.
- `git restore --staged .` - unstages other agents' work.
- `git stash` - stashes ALL changes including other agents' work.
- `git add -A` / `git add .` - stages other agents' uncommitted work.
- `git commit -am` - can commit tracked changes from other agents.
- `git commit --no-verify` - bypasses required checks and is never allowed.

## Conflicts

- If repository instructions and a direct user request conflict, pause and ask before overriding safety rules.
