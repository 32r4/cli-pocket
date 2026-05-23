import { defineConfig, type UserConfig } from "vite";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

export default defineConfig(({ mode }): UserConfig => {
  const isTauri = mode === "tauri";
  const wasmPackageDir = resolve(
    __dirname,
    "../../crates/client/client-core-wasm/pkg",
  );
  const wasmAlias = existsSync(resolve(wasmPackageDir, "package.json"))
    ? wasmPackageDir
    : resolve(__dirname, "src/bridge/wasmUnavailable.ts");

  return {
    resolve: {
      alias: {
        "@": resolve(__dirname, "src"),
        ...(isTauri ? {} : { "cli-pocket-client-core-wasm": wasmAlias }),
      },
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
