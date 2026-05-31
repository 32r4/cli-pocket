import type { ReactNode } from "react";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import type { OverlaySection } from "@/state/ui/uiState";

interface ControlOverlayProps {
	isOpen: boolean;
	isMobileUi: boolean;
	isMenuRoot: boolean;
	overlaySection: OverlaySection;
	detailSection: ReactNode;
	servers: DaemonRecord[];
	selectedServerId: string | null;
	onClose: () => void;
	onShowMenuRoot: () => void;
	onSelectSection: (section: OverlaySection) => void;
	onConnectServer: (server: DaemonRecord) => void;
	onOpenAddServer: () => void;
}

function BackIcon() {
	return <span className="back-button__icon" aria-hidden="true" />;
}

function sectionLabel(section: OverlaySection) {
	return section.charAt(0).toUpperCase() + section.slice(1);
}

function SavedServers({
	servers,
	selectedServerId,
	onConnectServer,
	onOpenAddServer,
	showSelection,
}: {
	servers: DaemonRecord[];
	selectedServerId: string | null;
	onConnectServer: (server: DaemonRecord) => void;
	onOpenAddServer: () => void;
	showSelection: boolean;
}) {
	return (
		<div className="server-list">
			<p className="server-list__heading">Saved servers</p>
			{servers.map((server) => (
				<button
					type="button"
					key={server.id}
					className="server-list__item"
					data-active={
						showSelection ? selectedServerId === server.id : undefined
					}
					onClick={() => onConnectServer(server)}
				>
					<span>{server.label}</span>
					<small>{server.kind === "direct" ? "Local" : "Remote"}</small>
				</button>
			))}
			<button
				type="button"
				className="server-list__add"
				onClick={onOpenAddServer}
			>
				+ Add server
			</button>
		</div>
	);
}

function OverlayNav({
	overlaySection,
	onSelectSection,
	showActiveState,
}: {
	overlaySection: OverlaySection;
	onSelectSection: (section: OverlaySection) => void;
	showActiveState: boolean;
}) {
	return (
		<nav className="overlay-nav" aria-label="Overlay sections">
			{(["settings", "diagnostics", "about"] as OverlaySection[]).map(
				(section) => (
					<button
						type="button"
						key={section}
						data-active={
							showActiveState ? overlaySection === section : undefined
						}
						onClick={() => onSelectSection(section)}
					>
						{sectionLabel(section)}
					</button>
				),
			)}
		</nav>
	);
}

export function ControlOverlay({
	isOpen,
	isMobileUi,
	isMenuRoot,
	overlaySection,
	detailSection,
	servers,
	selectedServerId,
	onClose,
	onShowMenuRoot,
	onSelectSection,
	onConnectServer,
	onOpenAddServer,
}: ControlOverlayProps) {
	if (!isOpen) {
		return null;
	}

	if (isMobileUi) {
		return (
			<aside
				className="control-overlay control-overlay--mobile"
				aria-label="Control overlay"
			>
				{!isMenuRoot ? (
					<div className="control-overlay__mobile-page">
						<button
							type="button"
							className="back-button"
							onClick={onShowMenuRoot}
							aria-label="Back to menu"
						>
							<BackIcon />
						</button>
						{detailSection}
					</div>
				) : (
					<div className="control-overlay__mobile-page">
						<button
							type="button"
							className="back-button"
							onClick={onClose}
							aria-label="Close menu"
						>
							<BackIcon />
						</button>
						<OverlayNav
							overlaySection={overlaySection}
							onSelectSection={onSelectSection}
							showActiveState={false}
						/>
						<div className="overlay-divider" aria-hidden="true" />
						<SavedServers
							servers={servers}
							selectedServerId={selectedServerId}
							onConnectServer={onConnectServer}
							onOpenAddServer={onOpenAddServer}
							showSelection={false}
						/>
					</div>
				)}
			</aside>
		);
	}

	return (
		<aside className="control-overlay" aria-label="Control overlay">
			<div className="control-overlay__rail">
				<button
					type="button"
					className="back-button"
					onClick={onClose}
					aria-label="Close menu"
				>
					<BackIcon />
				</button>
				<OverlayNav
					overlaySection={overlaySection}
					onSelectSection={onSelectSection}
					showActiveState
				/>
				<div className="overlay-divider" aria-hidden="true" />
				<SavedServers
					servers={servers}
					selectedServerId={selectedServerId}
					onConnectServer={onConnectServer}
					onOpenAddServer={onOpenAddServer}
					showSelection
				/>
			</div>
			<div className="control-overlay__detail">{detailSection}</div>
		</aside>
	);
}
