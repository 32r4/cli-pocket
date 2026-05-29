import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig(({ mode }) => {
	const isTauri = mode === "tauri" || mode === "mobile";
	const tauriDevHost = process.env.TAURI_DEV_HOST;
	const entryPath =
		mode === "web"
			? "/src/entries/web.tsx"
			: mode === "mobile"
				? "/src/entries/mobile.tsx"
				: "/src/entries/desktop.tsx";

	return {
		plugins: [
			react(),
			{
				name: "cli-pocket-entry-html",
				transformIndexHtml: {
					order: "pre",
					handler(html) {
						return html.replace("%APP_ENTRY%", entryPath);
					},
				},
			},
		],
		resolve: {
			alias: {
				"@": path.resolve(__dirname, "src"),
				"cli-pocket-client-core-wasm": path.resolve(
					__dirname,
					"../../crates/client/client-core-wasm/pkg",
				),
			},
		},
		define: {
			__CLIENT_KIND__: JSON.stringify(mode === "web" ? "web" : "tauri"),
			__MOBILE_ENTRY__: JSON.stringify(mode === "mobile"),
		},
		server: {
			...(isTauri ? { host: tauriDevHost ?? "0.0.0.0" } : {}),
			port: 5173,
			strictPort: true,
			fs: {
				allow: [path.resolve(__dirname, "../..")],
			},
		},
		build: {
			target: isTauri ? "chrome120" : "es2022",
			sourcemap: true,
			rollupOptions: {
				external:
					mode === "web" ? [/^@tauri-apps\//] : ["cli-pocket-client-core-wasm"],
			},
		},
		test: {
			environment: "jsdom",
			setupFiles: "./src/shared/test/setup.ts",
		},
	};
});
