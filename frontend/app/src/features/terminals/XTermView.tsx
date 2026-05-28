import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";

export function XTermView({ title }: { title: string }) {
	const hostRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		if (hostRef.current === null) {
			return;
		}

		if (import.meta.env.VITEST) {
			hostRef.current.textContent = `${title}\r\ncli-pocket`;
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
				if (cancelled || hostRef.current === null) {
					return;
				}

				terminal = new Terminal({
					cols: 120,
					rows: 32,
				});

				terminal.open(hostRef.current);
				terminal.write(`${title}\r\ncli-pocket\r\n`);
			})
			.catch(() => {
				if (hostRef.current !== null) {
					hostRef.current.textContent = "terminal unavailable";
				}
			});

		return () => {
			cancelled = true;
			terminal?.dispose();
		};
	}, [title]);

	return <div ref={hostRef} className="xterm-host" />;
}
