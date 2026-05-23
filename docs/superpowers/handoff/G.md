# Plan G Handoff: Webview Terminal

Date completed: 2026-05-23

## What Shipped

- `webview/terminal/`: a TypeScript/Vite subproject with dual build modes for Tauri and web.
- Scoped xterm packages:
  - `@xterm/xterm`
  - `@xterm/addon-fit`
  - `@xterm/addon-unicode11`
  - `@xterm/addon-webgl`
- `Renderer` wrapper around xterm.js with Fit, Unicode11, and best-effort WebGL addon setup.
- `ClientBridge` interface plus three implementations:
  - `TauriBridge` for Tauri command/event integration.
  - `WebBridge` for the wasm `CliPocketClient` API.
  - `MockBridge` for local browser development.
- Top-level UI:
  - `App`
  - `StatusBar`
  - `VirtualKeyBar`
  - `src/styles/app.css`
- Snapshot and delta helpers:
  - `src/render/snapshot.ts`
  - `src/render/delta.ts`
- Input helpers:
  - `src/input/keymap.ts`
  - `src/input/paste.ts`
- Type definitions matching the current proto JSON shape:
  - `src/types/frame.ts`
  - `src/types/events.ts`
- Vitest coverage for app wiring, bridge behavior, renderer behavior, snapshot/delta application, keymap, paste, and mock/dev entry behavior.

## Output Bundles

- Tauri bundle: `webview/terminal/dist/tauri`
  - Built with `cd webview/terminal && npm run build:tauri`.
- Web bundle: `webview/terminal/dist/web`
  - Built with `cd webview/terminal && npm run build:web`.
- Combined build:
  - `cd webview/terminal && npm run build`.

## Downstream Notes

- Tauri commands are snake_case aliases, not colon-delimited names:
  - `cli_pocket_connect`
  - `cli_pocket_create_terminal`
  - `cli_pocket_send_input`
  - `cli_pocket_resize`
  - `cli_pocket_kill`
  - `cli_pocket_export_identity`
  - `cli_pocket_import_identity`
  - `cli_pocket_close`
- The Tauri event channel is `cli_pocket:event`.
- `ClientBridge.createTerminal` is request-only. The terminal id does not come back from the call. It arrives later in a `TerminalCreated` event at `TerminalCreated.info.terminal`.
- The current `TerminalInfo` shape is:
  - `terminal`
  - `cols`
  - `rows`
  - `created_at_unix_ms`
  - `label`
  - `attached_clients`
- `WebBridge` keeps the wasm source import literal:
  - `import("cli-pocket-client-core-wasm")`
- Vite web mode aliases `cli-pocket-client-core-wasm` to `crates/client/client-core-wasm/pkg` when that generated package exists. If it does not exist, the alias points to `src/bridge/wasmUnavailable.ts` until Plan I builds and links wasm.
- Mock development:
  - `cd webview/terminal && npm run dev`
  - Open `http://localhost:5173/?mock=1`.
- Plan I should expect current wasm compatibility shims in `WebBridge`:
  - Current Plan F methods can target the active terminal only.
  - Future methods may accept `terminalId`; `WebBridge` checks function arity for `send_input`, `resize`, and `kill`.

## Deviations From Original Plan G Examples

- The xterm packages are the scoped `@xterm/*` packages, not the older unscoped `xterm*` package names.
- Tauri commands use snake_case aliases (`cli_pocket_connect`) instead of original example names like `cli_pocket:connect`.
- `TerminalCreated.info` uses the current proto shape and stores the terminal id in `info.terminal`, not `info.terminal_id`.
- `createTerminal` is event-driven. It returns `Promise<void>` and the `TerminalCreated` event carries the created terminal metadata.
- The web wasm import remains a literal package import, with Vite resolving it through mode-specific aliasing.
- Identity export/import use bytes at the `ClientBridge` boundary (`Uint8Array`), with normalization for wasm/Tauri return shapes.

## Validation

Plan G commits present in this branch:

- `e5a53be` `feat(webview): subproject scaffold with dual Vite build`
- `2d8d170` `feat(webview): wire-shape type definitions`
- `155e66a` `feat(webview): ClientBridge interface + client-kind constant`
- `b47518d` `feat(webview): TauriBridge using invoke + listen`
- `f3c3fb1` `feat(webview): WebBridge using wasm CliPocketClient`
- `c23219e` `feat(webview): snapshot + delta apply with vitest`
- `d19ef70` `feat(webview): xterm Renderer wrapper`
- `e37d390` `feat(webview): keymap + paste helpers`
- `fdca6fc` `feat(webview): App + StatusBar + VirtualKeyBar`
- `2475828` `feat(webview): MockBridge for ?mock=1 dev story`

Validation commands used during Plan G, based on the shipped task history and package scripts:

- `cd webview/terminal && npm run typecheck` passed during the type/bridge/UI tasks.
- `cd webview/terminal && npm run lint` passed as part of webview checks.
- `cd webview/terminal && npm run test` passed with Vitest coverage for the webview package.
- `cd webview/terminal && npm run check` passed for the webview package.
- `cd webview/terminal && npm run build:tauri` passed and writes `dist/tauri`.
- `cd webview/terminal && npm run build:web` passed and writes `dist/web`, using the generated wasm package when present or `wasmUnavailable.ts` otherwise.

Known repository-level validation blocker:

- `just check` currently fails on pre-existing Rust formatting in `crates/server/daemon-core/src/server.rs`.
- This is not introduced by Plan G.
- Webview package checks and builds pass.

## Suggested Next Skills

- For Plan H Tauri integration: use `diagnose` if command/event wiring fails, otherwise implement against `TauriBridge.ts` and `src/types/events.ts`.
- For Plan I web integration: use `test-driven-development` around wasm event shape compatibility and `browser:browser` for local web smoke checks.
