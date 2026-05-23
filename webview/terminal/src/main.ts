import { App } from "@/ui/App";
import { CLIENT_KIND, type ClientBridge } from "@/bridge/ClientBridge";
import { TauriBridge } from "@/bridge/TauriBridge";
import { WebBridge } from "@/bridge/WebBridge";
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

async function createBridge(): Promise<ClientBridge> {
  if (CLIENT_KIND === "tauri") {
    return new TauriBridge();
  }

  const params = new URLSearchParams(window.location.search);
  if (params.get("mock") === "1") {
    throw new Error("mock bridge is not implemented");
  }

  return WebBridge.create();
}
