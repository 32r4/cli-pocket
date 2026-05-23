import { App } from "@/ui/App";
import {
  CLIENT_KIND,
  type ClientBridge,
  type ConnectConfig,
} from "@/bridge/ClientBridge";
import { MockBridge } from "@/bridge/MockBridge";
import { TauriBridge } from "@/bridge/TauriBridge";
import "@/styles/app.css";

const root = document.getElementById("app");

if (root === null) {
  throw new Error("missing #app root");
}

const bridge = await createBridge();
const app = new App(root, bridge, CLIENT_KIND);
await app.start();

window.addEventListener("beforeunload", () => {
  void app.dispose();
});

await bootstrapConnect(app, bridge);

async function createBridge(): Promise<ClientBridge> {
  const params = new URLSearchParams(window.location.search);
  if (params.get("mock") === "1") {
    const bridge = new MockBridge();
    await bridge.connect({
      endpointUrl: "mock://cli-pocket",
      serverPublicHex: "mock",
    });
    return bridge;
  }

  if (CLIENT_KIND === "tauri") {
    return new TauriBridge();
  }

  // Dynamic import to avoid bundling wasm in tauri mode
  const { WebBridge } = await import("@/bridge/WebBridge");
  return WebBridge.create();
}

async function bootstrapConnect(app: App, bridge: ClientBridge): Promise<void> {
  const params = new URLSearchParams(window.location.search);
  if (params.get("mock") === "1") {
    return;
  }

  const config = connectConfigFromUrl();
  if (config === null) {
    // Plan H/I may provide pairing or connection details after the app mounts.
    return;
  }

  try {
    await bridge.connect(config);
  } catch (error) {
    const message = errorMessage(error);
    console.error("failed to connect from URL parameters", error);
    app.showError(message);
  }
}

function connectConfigFromUrl(): ConnectConfig | null {
  const params = new URLSearchParams(window.location.search);
  const endpointUrl = params.get("endpointUrl") ?? params.get("endpoint_url");
  const serverPublicHex =
    params.get("serverPublicHex") ?? params.get("server_public_hex");
  const resumeTokenHex =
    params.get("resumeTokenHex") ?? params.get("resume_token_hex");

  if (endpointUrl === null || serverPublicHex === null) {
    return null;
  }

  return {
    endpointUrl,
    serverPublicHex,
    ...(resumeTokenHex === null ? {} : { resumeTokenHex }),
  };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
