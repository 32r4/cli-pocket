declare const __CLIENT_KIND__: "tauri" | "web";

const root = document.getElementById("root");
if (root) {
  root.textContent = `cli-pocket webview (kind=${__CLIENT_KIND__})`;
}
