import { Cloud, Monitor, Plus, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import type { MenuSection } from "@/state/ui/uiState";

interface ControlOverlayProps {
	isMobileUi: boolean;
	isMenuRoot: boolean;
	menuSection: MenuSection;
	detailSection: ReactNode;
	servers: DaemonRecord[];
	selectedServerId: string | null;
	onSelectSection: (section: MenuSection) => void;
	onConnectServer: (server: DaemonRecord) => void;
	onDeleteServer: (serverId: string) => void;
	onOpenAddServer: () => void;
}

function sectionLabel(section: MenuSection) {
	return section.charAt(0).toUpperCase() + section.slice(1);
}

function ServerKindIcon({ kind }: { kind: DaemonRecord["kind"] }) {
	return kind === "direct" ? (
		<Monitor aria-hidden="true" size={14} strokeWidth={1.75} />
	) : (
		<Cloud aria-hidden="true" size={14} strokeWidth={1.75} />
	);
}

function SavedServers({
	servers,
	selectedServerId,
	onConnectServer,
	onDeleteServer,
	onOpenAddServer,
	showSelection,
}: {
	servers: DaemonRecord[];
	selectedServerId: string | null;
	onConnectServer: (server: DaemonRecord) => void;
	onDeleteServer: (serverId: string) => void;
	onOpenAddServer: () => void;
	showSelection: boolean;
}) {
	return (
		<div className="server-list">
			{servers.map((server) => (
				<div className="server-list__row" key={server.id}>
					<button
						type="button"
						className="server-list__item"
						data-active={
							showSelection ? selectedServerId === server.id : undefined
						}
						onClick={() => onConnectServer(server)}
					>
						<ServerKindIcon kind={server.kind} />
						<span className="server-list__label">{server.label}</span>
						<span className="sr-only">
							{server.kind === "direct" ? "Local server" : "Remote server"}
						</span>
					</button>
					{server.id !== "local-daemon" ? (
						<button
							type="button"
							className="icon-button server-list__delete"
							aria-label={`Delete ${server.label}`}
							onClick={(event) => {
								event.stopPropagation();
								onDeleteServer(server.id);
							}}
						>
							<Trash2 aria-hidden="true" size={14} strokeWidth={1.75} />
						</button>
					) : null}
				</div>
			))}
			<button
				type="button"
				className="server-list__add"
				onClick={onOpenAddServer}
			>
				<Plus aria-hidden="true" size={14} strokeWidth={1.75} />
				<span>Add server</span>
			</button>
		</div>
	);
}

function OverlayNav({
	menuSection,
	onSelectSection,
	showActiveState,
}: {
	menuSection: MenuSection;
	onSelectSection: (section: MenuSection) => void;
	showActiveState: boolean;
}) {
	return (
		<nav className="overlay-nav" aria-label="Overlay sections">
			{(["settings", "diagnostics", "about"] as MenuSection[]).map(
				(section) => (
					<button
						type="button"
						key={section}
						data-active={showActiveState ? menuSection === section : undefined}
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
	isMobileUi,
	isMenuRoot,
	menuSection,
	detailSection,
	servers,
	selectedServerId,
	onSelectSection,
	onConnectServer,
	onDeleteServer,
	onOpenAddServer,
}: ControlOverlayProps) {
	if (isMobileUi) {
		return (
			<section
				className="control-overlay control-overlay--mobile"
				aria-label="Control overlay"
			>
				{!isMenuRoot ? (
					<div className="control-overlay__mobile-page">{detailSection}</div>
				) : (
					<div className="control-overlay__mobile-page">
						<OverlayNav
							menuSection={menuSection}
							onSelectSection={onSelectSection}
							showActiveState={false}
						/>
						<div className="overlay-divider" aria-hidden="true" />
						<SavedServers
							servers={servers}
							selectedServerId={selectedServerId}
							onConnectServer={onConnectServer}
							onDeleteServer={onDeleteServer}
							onOpenAddServer={onOpenAddServer}
							showSelection={false}
						/>
					</div>
				)}
			</section>
		);
	}

	return (
		<section className="control-overlay" aria-label="Control overlay">
			<div className="control-overlay__rail">
				<OverlayNav
					menuSection={menuSection}
					onSelectSection={onSelectSection}
					showActiveState
				/>
				<div className="overlay-divider" aria-hidden="true" />
				<SavedServers
					servers={servers}
					selectedServerId={selectedServerId}
					onConnectServer={onConnectServer}
					onDeleteServer={onDeleteServer}
					onOpenAddServer={onOpenAddServer}
					showSelection
				/>
			</div>
			<div className="control-overlay__detail">{detailSection}</div>
		</section>
	);
}
