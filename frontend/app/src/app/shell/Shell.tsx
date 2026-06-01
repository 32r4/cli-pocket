import { Menu } from "lucide-react";
import type { ReactNode } from "react";

interface ShellProps {
	children: ReactNode;
	activeServerLabel: string | null;
	connectionState: "idle" | "connecting" | "connected" | "failed";
	isOverlayOpen: boolean;
	onOpenOverlay: () => void;
	onCloseOverlay: () => void;
}

export function Shell({
	children,
	activeServerLabel,
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
					<Menu aria-hidden="true" size={18} strokeWidth={1.75} />
				</button>
				<div className="app-shell__host">
					<strong>{activeServerLabel ?? "No server"}</strong>
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
