import { ArrowLeft, Menu, Minus, Square, X } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import type { DesktopWindowControls } from "./useDesktopWindowControls";

type PrimaryNavigationMode = "menu" | "back";

interface ShellProps {
	children: ReactNode;
	activeServerLabel: string | null;
	connectionState: "idle" | "connecting" | "connected" | "failed";
	windowControls: DesktopWindowControls | null;
	primaryNavigationMode: PrimaryNavigationMode;
	onPrimaryNavigation: () => void;
}

export function Shell({
	children,
	activeServerLabel,
	connectionState,
	windowControls,
	primaryNavigationMode,
	onPrimaryNavigation,
}: ShellProps) {
	const isDesktopWindow = windowControls != null;
	const indicatorState =
		connectionState === "connected"
			? "green"
			: connectionState === "connecting"
				? "yellow"
				: "red";
	const headerRef = useRef<HTMLElement | null>(null);

	useEffect(() => {
		const header = headerRef.current;
		if (!isDesktopWindow || header == null) {
			return;
		}

		const isWindowButton = (target: EventTarget | null) =>
			target instanceof Element && target.closest("button") != null;

		const handleMouseDown = (event: MouseEvent) => {
			if (event.button !== 0 || event.detail > 1) {
				return;
			}
			if (isWindowButton(event.target)) {
				return;
			}

			windowControls.startDragging();
		};

		const handleDoubleClick = (event: MouseEvent) => {
			if (isWindowButton(event.target)) {
				return;
			}

			windowControls.toggleMaximize();
		};

		header.addEventListener("mousedown", handleMouseDown);
		header.addEventListener("dblclick", handleDoubleClick);
		return () => {
			header.removeEventListener("mousedown", handleMouseDown);
			header.removeEventListener("dblclick", handleDoubleClick);
		};
	}, [isDesktopWindow, windowControls]);

	return (
		<div
			className="app-shell"
			data-window-chrome={isDesktopWindow ? "desktop" : "standard"}
		>
			<header className="app-shell__header" ref={headerRef}>
				<button
					type="button"
					className="icon-button"
					onMouseDown={(event) => {
						event.stopPropagation();
					}}
					onDoubleClick={(event) => {
						event.stopPropagation();
					}}
					onClick={onPrimaryNavigation}
					aria-label={
						primaryNavigationMode === "back"
							? "Go back"
							: "Open control overlay"
					}
				>
					{primaryNavigationMode === "back" ? (
						<ArrowLeft aria-hidden="true" size={16} strokeWidth={1.75} />
					) : (
						<Menu aria-hidden="true" size={16} strokeWidth={1.75} />
					)}
				</button>
				<div className="app-shell__host">
					{activeServerLabel != null ? (
						<strong>{activeServerLabel}</strong>
					) : null}
					<span className="sr-only">{`Status ${indicatorState}`}</span>
					<span
						className="app-shell__status-light"
						data-state={indicatorState}
					/>
				</div>
				{isDesktopWindow ? (
					<div className="app-shell__window-controls">
						<button
							type="button"
							className="icon-button app-shell__window-button"
							aria-label="Minimize window"
							onClick={() => {
								windowControls.minimize();
							}}
						>
							<Minus aria-hidden="true" size={14} strokeWidth={1.75} />
						</button>
						<button
							type="button"
							className="icon-button app-shell__window-button"
							aria-label="Toggle maximize window"
							onClick={() => {
								windowControls.toggleMaximize();
							}}
						>
							<Square aria-hidden="true" size={14} strokeWidth={1.75} />
						</button>
						<button
							type="button"
							className="icon-button app-shell__window-button app-shell__window-button--danger"
							aria-label="Close window"
							onClick={() => {
								windowControls.close();
							}}
						>
							<X aria-hidden="true" size={14} strokeWidth={1.75} />
						</button>
					</div>
				) : null}
			</header>
			{children}
		</div>
	);
}
