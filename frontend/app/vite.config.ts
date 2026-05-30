import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const APP_PLATFORM_BY_MODE = {
	web: "web",
	mobile: "mobile",
	desktop: "desktop",
	tauri: "desktop",
} as const;

const DEV_PORT_BY_MODE = {
	web: 5175,
	mobile: 5174,
	desktop: 5173,
	tauri: 5173,
} as const;

export default defineConfig(({ mode }) => {
	const isTauri = mode in APP_PLATFORM_BY_MODE && mode !== "web";
	const tauriDevHost = process.env.TAURI_DEV_HOST;
	const appPlatform =
		APP_PLATFORM_BY_MODE[mode as keyof typeof APP_PLATFORM_BY_MODE] ??
		"desktop";
	const devPort =
		DEV_PORT_BY_MODE[mode as keyof typeof DEV_PORT_BY_MODE] ?? 5173;
	const entryPath = "/src/entries/main.tsx";

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
			__APP_PLATFORM__: JSON.stringify(appPlatform),
		},
		server: {
			...(isTauri ? { host: tauriDevHost ?? "0.0.0.0" } : {}),
			port: devPort,
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
