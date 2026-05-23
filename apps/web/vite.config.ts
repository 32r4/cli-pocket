import { defineConfig } from "vite";
import path from "node:path";

export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "../../webview/terminal/src"),
      "@web": path.resolve(__dirname, "src"),
      "@terminal": path.resolve(__dirname, "../../webview/terminal/src"),
      "cli-pocket-client-core-wasm": path.resolve(
        __dirname,
        "../../crates/client/client-core-wasm/pkg/cli_pocket_client_core_wasm.js",
      ),
    },
  },
  define: {
    __CLIENT_KIND__: JSON.stringify("web"),
  },
  server: {
    port: 5174,
    strictPort: true,
    fs: {
      allow: [path.resolve(__dirname, "../..")],
    },
  },
  build: {
    target: "es2022",
    sourcemap: true,
    rollupOptions: {
      external: [/^@tauri-apps\//],
    },
  },
});
