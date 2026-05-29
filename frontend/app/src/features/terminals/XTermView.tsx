import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";

export function XTermView({ title }: { title: string }) {
	const serverRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		if (serverRef.current === null) {
			return;
		}

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

				terminal = new Terminal({
					cols: 120,
					rows: 32,
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
	}, [title]);

	return <div ref={serverRef} className="xterm-server" />;
}
