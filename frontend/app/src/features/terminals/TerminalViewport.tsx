import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import type { TerminalSessionRegistry } from "./terminalSessionRegistry";

export function TerminalViewport({
	registry,
}: {
	registry: TerminalSessionRegistry;
}) {
	const hostRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		const host = hostRef.current;
		if (host == null) {
			return;
		}

		void registry.mountActive(host);
		return () => {
			registry.unmountActive();
		};
	}, [registry]);

	return (
		<div className="xterm-shell">
			<div ref={hostRef} className="xterm-host" />
		</div>
	);
}
