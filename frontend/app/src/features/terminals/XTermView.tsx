import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import type { ThemeName } from "@/state/ui/uiState";

function readThemeToken(name: string) {
	if (typeof window === "undefined") {
		return "";
	}

	return getComputedStyle(document.documentElement)
		.getPropertyValue(name)
		.trim();
}

export function XTermView({
	title,
	theme,
}: {
	title: string;
	theme: ThemeName;
}) {
	const serverRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		if (serverRef.current === null) {
			return;
		}

		serverRef.current.dataset.theme = theme;

		if (import.meta.env.VITEST) {
			serverRef.current.textContent = `${title}\r\ncli-pocket`;
			return;
		}

		let terminal: {
			open: (node: HTMLElement) => void;
			write: (data: string) => void;
			dispose: () => void;
		} | null = null;
		let cancelled = false;

		void import("@xterm/xterm")
			.then(({ Terminal }) => {
				if (cancelled || serverRef.current === null) {
					return;
				}

				const terminalTheme = {
					background: readThemeToken("--surface-terminal"),
					foreground: readThemeToken("--terminal-fg"),
					cursor: readThemeToken("--terminal-cursor"),
					selectionBackground: readThemeToken("--terminal-selection-bg"),
				};

				terminal = new Terminal({
					cols: 120,
					rows: 32,
					theme: terminalTheme,
				});

				terminal.open(serverRef.current);
				terminal.write(`${title}\r\ncli-pocket\r\n`);
			})
			.catch(() => {
				if (serverRef.current !== null) {
					serverRef.current.textContent = "terminal unavailable";
				}
			});

		return () => {
			cancelled = true;
			terminal?.dispose();
		};
	}, [theme, title]);

	return <div ref={serverRef} className="xterm-server" />;
}
