import { defineConfig } from "vite";

export default defineConfig({
  build: {
    target: "es2022",
    sourcemap: true,
  },
  define: {
    __CLIENT_KIND__: JSON.stringify(process.env.VITE_CLIENT_KIND ?? "tauri"),
  },
});
