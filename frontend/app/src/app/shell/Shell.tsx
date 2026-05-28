import type { ReactNode } from "react";

interface ShellProps {
	children: ReactNode;
	clientKind: "web" | "tauri";
	statusText: string;
	activeHostLabel: string | null;
	activeEndpoint: string | null;
	isOverlayOpen: boolean;
	onOpenOverlay: () => void;
	onCloseOverlay: () => void;
}

export function Shell({
	children,
	clientKind,
	statusText,
	activeHostLabel,
	activeEndpoint,
	isOverlayOpen,
	onOpenOverlay,
	onCloseOverlay,
}: ShellProps) {
	return (
		<div className="app-shell">
			<header className="app-shell__header">
				<button
					type="button"
					className="menu-button"
					onClick={isOverlayOpen ? onCloseOverlay : onOpenOverlay}
					aria-label={
						isOverlayOpen ? "Close control overlay" : "Open control overlay"
					}
				>
					{isOverlayOpen ? "Close" : "Menu"}
				</button>
				<div className="app-shell__brand">
					<h1>cli-pocket</h1>
					<p>{clientKind}</p>
				</div>
				<div className="app-shell__status">
					<strong>{statusText}</strong>
					<span>{activeHostLabel ?? "No host"}</span>
					<span>{activeEndpoint ?? "Not connected"}</span>
				</div>
			</header>
			{children}
		</div>
	);
}
