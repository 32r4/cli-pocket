import { defineConfig, type UserConfig } from "vite";
import { resolve } from "node:path";

export default defineConfig(({ mode }): UserConfig => {
  const isTauri = mode === "tauri";
  return {
    resolve: {
      alias: { "@": resolve(__dirname, "src") },
    },
    define: {
      __CLIENT_KIND__: JSON.stringify(isTauri ? "tauri" : "web"),
    },
    build: {
      target: isTauri ? "chrome120" : "es2022",
      sourcemap: true,
      rollupOptions: {
        external: isTauri
          ? ["cli-pocket-client-core-wasm"]
          : ["@tauri-apps/api", "@tauri-apps/api/core", "@tauri-apps/api/event"],
      },
    },
    server: {
      port: 5173,
      strictPort: true,
    },
  };
});
