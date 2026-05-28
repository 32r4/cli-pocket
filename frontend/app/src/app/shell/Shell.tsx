import type { ReactNode } from "react";

interface ShellProps {
	children: ReactNode;
	activeHostLabel: string | null;
	connectionState: "idle" | "connecting" | "connected" | "failed";
	isOverlayOpen: boolean;
	onOpenOverlay: () => void;
	onCloseOverlay: () => void;
}

export function Shell({
	children,
	activeHostLabel,
	connectionState,
	isOverlayOpen,
	onOpenOverlay,
	onCloseOverlay,
}: ShellProps) {
	const indicatorState =
		connectionState === "connected"
			? "green"
			: connectionState === "connecting"
				? "yellow"
				: "red";

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
					<span className="menu-button__icon" aria-hidden="true">
						<span />
						<span />
						<span />
					</span>
				</button>
				<div className="app-shell__host">
					<strong>{activeHostLabel ?? "No host"}</strong>
					<span className="sr-only">{`Status ${indicatorState}`}</span>
					<span
						className="app-shell__status-light"
						data-state={indicatorState}
					/>
				</div>
			</header>
			{children}
		</div>
	);
}
