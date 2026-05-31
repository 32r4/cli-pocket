import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import type { TerminalController } from "./terminalController";

export function TerminalViewport({
	controller,
}: {
	controller: TerminalController;
}) {
	const hostRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		const host = hostRef.current;
		if (host == null) {
			return;
		}

		void controller.mount(host);
		return () => {
			controller.unmount();
		};
	}, [controller]);

	return (
		<div className="xterm-shell">
			<div ref={hostRef} className="xterm-host" />
		</div>
	);
}
