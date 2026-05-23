import { defineConfig } from "vite";
import path from "node:path";

export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
      "@terminal": path.resolve(__dirname, "../../webview/terminal/src"),
    },
  },
  define: {
    __CLIENT_KIND__: JSON.stringify("web"),
  },
  server: {
    port: 5174,
    strictPort: true,
  },
  build: {
    target: "es2022",
    sourcemap: true,
    rollupOptions: {
      external: [/^@tauri-apps\//],
    },
  },
});
