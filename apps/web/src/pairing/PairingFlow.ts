// PairingFlow.ts — web app entry point: SPAKE2 pairing + localStorage server selector.
//
// Signature notes (verified by reading upstream sources):
//   WebBridge: constructor(client: WasmClient) — or static WebBridge.create(client?)
//   App:       constructor(host: HTMLElement, bridge: ClientBridge, clientKind: "tauri"|"web")
//              then call .start(): Promise<void>
//   ConnectConfig: { endpointUrl, serverPublicHex, resumeTokenHex? } (camelCase)

import init, { client_pair_with_code } from "cli-pocket-client-core-wasm";
import { installHashHandlers } from "@web/identity/IdentityActions";
import { mountPairingView, type PairingValues } from "./PairingView";
import type { SavedServer } from "./relayEndpoint";

const STORE_KEY = "cli-pocket/server-selector/v1";

function loadSaved(): SavedServer | null {
  const raw = localStorage.getItem(STORE_KEY);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as SavedServer;
  } catch {
    return null;
  }
}

function saveSaved(s: SavedServer): void {
  localStorage.setItem(STORE_KEY, JSON.stringify(s));
}

function clearSaved(): void {
  localStorage.removeItem(STORE_KEY);
}

export async function startWebApp(root: HTMLElement): Promise<void> {
  await init();

  const saved = loadSaved();
  if (saved) {
    try {
      await launchTerminal(root, saved);
      return;
    } catch {
      clearSaved();
    }
  }

  mountPairing(root);
}

function mountPairing(root: HTMLElement): void {
  mountPairingView(root, async (v: PairingValues) => {
    const result = (await client_pair_with_code(v.daemon_pairing_url, v.code)) as {
      server_public_hex: string;
      client_public_hex: string;
    };
    const sel: SavedServer = {
      endpoint_url: v.daemon_pairing_url, // For v1, reconnect to the same daemon URL
      server_public_hex: result.server_public_hex,
      client_public_hex: result.client_public_hex,
      resume_token_hex: null,
    };
    saveSaved(sel);
    try {
      await launchTerminal(root, sel);
    } catch (err) {
      clearSaved();
      throw err;
    }
  });
}

async function launchTerminal(root: HTMLElement, saved: SavedServer): Promise<void> {
  // Use WebBridge.create() — the static factory handles wasm module loading and
  // constructs the CliPocketClient internally (wasm init already called above).
  const { WebBridge } = await import("@terminal/bridge/WebBridge");
  const bridge = await WebBridge.create();

  // ConnectConfig uses camelCase fields (verified in ClientBridge.ts):
  //   endpointUrl, serverPublicHex, resumeTokenHex (optional)
  await bridge.connect({
    endpointUrl: saved.endpoint_url,
    serverPublicHex: saved.server_public_hex,
    resumeTokenHex: saved.resume_token_hex ?? undefined,
  });

  // Wire #export / #import URL-hash handlers so the user can export/import
  // their identity without navigating away from the terminal.
  // WebBridge satisfies IdentityClient (exportIdentity + importIdentity).
  installHashHandlers(bridge);

  // App constructor: (host, bridge, clientKind) — then call .start()
  // (verified in webview/terminal/src/ui/App.ts)
  const { App } = await import("@terminal/ui/App");
  const app = new App(root, bridge, "web");
  await app.start();
}
