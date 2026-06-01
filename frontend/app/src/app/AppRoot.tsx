import { useEffect, useState } from "react";
import { useStore } from "zustand";
import type { PlatformServices } from "@/platform/bridge/types";
import {
	type AppPlatform,
	createPlatformServices,
} from "@/platform/runtime/platform";
import { ErrorBanner } from "@/shared/components/ErrorBanner";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { ControlOverlay } from "./ControlOverlay";
import { HostSettingsSection } from "./HostSettingsSection";
import {
	initialFormState,
	makeServerRecord,
	type ServerFormState,
	ServerModal,
	type ServerModalMode,
} from "./ServerModal";
import { Shell } from "./shell/Shell";
import { createAppStores } from "./stores";
import { TerminalArea } from "./terminalArea";
import { useAppRuntime } from "./useAppRuntime";

interface AppRootProps {
	platform: AppPlatform;
	platformServicesFactory?: (
		platform: AppPlatform,
	) => Promise<PlatformServices>;
}

function endpointLabel(server: DaemonRecord) {
	return server.kind === "direct" ? server.endpointUrl : server.relayUrl;
}

export function AppRoot({
	platform,
	platformServicesFactory = createPlatformServices,
}: AppRootProps) {
	const [stores] = useState(() => createAppStores());
	const { daemonRegistry, uiState, workspaceState } = stores;
	const registry = useStore(daemonRegistry);
	const ui = useStore(uiState);
	const workspace = useStore(workspaceState);
	const [serverModalMode, setServerModalMode] =
		useState<ServerModalMode>("closed");
	const [serverForm, setServerForm] = useState<ServerFormState>(() =>
		initialFormState(),
	);
	const [pairingUrl, setPairingUrl] = useState("");
	const [inlineError, setInlineError] = useState<string | null>(null);
	const [isNarrowViewport, setIsNarrowViewport] = useState(() =>
		typeof window !== "undefined" ? window.innerWidth <= 900 : false,
	);
	const {
		services,
		platformError,
		localPairUrl,
		session,
		terminalController,
		connectServer,
		disconnectCurrentServer,
		generateLocalPairUrl,
		restartLocalDaemon,
		importPairingLink: importAndConnectPairingLink,
	} = useAppRuntime({
		platform,
		platformServicesFactory,
		stores,
		onInlineError: setInlineError,
	});

	const selectedServer =
		registry.daemons.find((daemon) => daemon.id === ui.selectedServerId) ??
		null;
	const activeServer =
		registry.daemons.find(
			(daemon) => daemon.id === workspace.activeConnectionServerId,
		) ?? null;
	const mainServer =
		selectedServer ?? activeServer ?? registry.daemons[0] ?? null;

	useEffect(() => {
		if (typeof window === "undefined") {
			return;
		}

		const updateViewport = () => {
			setIsNarrowViewport(window.innerWidth <= 900);
		};

		updateViewport();
		window.addEventListener("resize", updateViewport);
		return () => {
			window.removeEventListener("resize", updateViewport);
		};
	}, []);

	const activeSession =
		workspace.terminals.find(
			(terminal) => terminal.id === workspace.activeSessionId,
		) ?? null;

	const closeServerModal = () => {
		setServerModalMode("closed");
		setInlineError(null);
	};

	const openAddServerChooser = () => {
		setInlineError(null);
		setPairingUrl("");
		setServerModalMode("chooser");
	};

	const openDirectServerModal = () => {
		setInlineError(null);
		setServerForm(initialFormState());
		setPairingUrl("");
		setServerModalMode("direct");
	};

	const openPairingServerModal = () => {
		setInlineError(null);
		setPairingUrl("");
		setServerModalMode("pairing");
	};

	const saveServer = () => {
		const nextServer = makeServerRecord(serverForm);
		daemonRegistry.getState().upsertDaemon(nextServer);
		daemonRegistry.getState().selectDaemon(nextServer.id);
		uiState.getState().setSelectedServerId(nextServer.id);
		closeServerModal();
	};

	const deleteServer = async (serverId: string) => {
		const workspaceSnapshot = workspaceState.getState();
		const shouldDisconnect =
			workspaceSnapshot.activeConnectionServerId === serverId;
		if (shouldDisconnect) {
			await disconnectCurrentServer();
		}

		daemonRegistry.getState().removeDaemon(serverId);
		const nextSelectedServerId =
			uiState.getState().selectedServerId === serverId
				? daemonRegistry.getState().selectedDaemonId
				: uiState.getState().selectedServerId;
		uiState.getState().setSelectedServerId(nextSelectedServerId);
	};

	const importPairingLink = async () => {
		try {
			await importAndConnectPairingLink(pairingUrl);
			setPairingUrl("");
			closeServerModal();
		} catch (error: unknown) {
			setInlineError(
				error instanceof Error
					? error.message
					: "failed to import pairing link",
			);
		}
	};

	const errorMessage = inlineError ?? workspace.lastError ?? platformError;
	const hasSavedServers = registry.daemons.length > 0;
	const isMobileUi = platform.shell === "mobile" || isNarrowViewport;
	const overlayDetailSection =
		ui.overlaySection === "settings" ? (
			<HostSettingsSection
				hostAvailable={services?.host != null}
				localPairUrl={localPairUrl}
				theme={ui.theme}
				onGenerateLocalPairUrl={generateLocalPairUrl}
				onRestartLocalDaemon={restartLocalDaemon}
				onThemeChange={(theme) => ui.setTheme(theme)}
			/>
		) : ui.overlaySection === "diagnostics" ? (
			<section className="detail-section">
				<h2>Diagnostics</h2>
				<div className="detail-grid">
					<div>
						<span>Status</span>
						<strong>{workspace.connectionState}</strong>
					</div>
					<div>
						<span>Active server</span>
						<strong>{activeServer?.label ?? "none"}</strong>
					</div>
					<div>
						<span>Endpoint</span>
						<strong>
							{activeServer ? endpointLabel(activeServer) : "none"}
						</strong>
					</div>
					<div>
						<span>Last error</span>
						<strong>{workspace.lastError ?? "none"}</strong>
					</div>
					<div>
						<span>Client</span>
						<strong>{platform.id}</strong>
					</div>
					<div>
						<span>Active terminal</span>
						<strong>{activeSession?.title ?? "none"}</strong>
					</div>
					<div>
						<span>Terminal count</span>
						<strong>{workspace.terminals.length}</strong>
					</div>
				</div>
				<div className="action-row">
					<button type="button" disabled>
						Copy diagnostics
					</button>
					<button
						type="button"
						onClick={() => {
							workspaceState.getState().clearError();
							setInlineError(null);
						}}
					>
						Clear errors
					</button>
				</div>
			</section>
		) : (
			<section className="detail-section">
				<h2>About</h2>
				<div className="detail-grid">
					<div>
						<span>Version</span>
						<strong>0.1.0</strong>
					</div>
					<div>
						<span>Client</span>
						<strong>{platform.id}</strong>
					</div>
					<div>
						<span>Protocol</span>
						<strong>v1</strong>
					</div>
				</div>
				<p>Self-hosted remote terminal client.</p>
			</section>
		);

	return (
		<Shell
			activeServerLabel={activeServer?.label ?? null}
			connectionState={workspace.connectionState}
			isOverlayOpen={ui.isOverlayOpen}
			onOpenOverlay={() => ui.openOverlay("settings")}
			onCloseOverlay={ui.closeOverlay}
		>
			<main className="app-shell__main">
				{services == null ? (
					<section
						className="connection-status-panel"
						aria-label="Restoring saved servers"
					>
						<h2>Loading servers</h2>
						<p>Restoring saved servers.</p>
					</section>
				) : workspace.connectionState === "connected" ? (
					<TerminalArea
						session={session}
						workspace={workspace}
						workspaceState={workspaceState}
						controller={terminalController}
						theme={ui.theme}
						onInlineError={setInlineError}
					/>
				) : hasSavedServers ? (
					<section
						className="connection-status-panel"
						aria-label="Connection status"
					>
						<h2>
							{workspace.connectionState === "failed"
								? "Connection failed"
								: "Connecting"}
						</h2>
						<p>{mainServer?.label ?? "No server"}</p>
					</section>
				) : (
					<section
						className="empty-hosts-panel"
						aria-label="Add server options"
					>
						<button type="button" onClick={openDirectServerModal}>
							Direct connection
						</button>
						<button type="button" onClick={openPairingServerModal}>
							Pairing link
						</button>
						<button type="button" disabled>
							QR code
						</button>
					</section>
				)}

				<ErrorBanner message={errorMessage} />
			</main>

			<ControlOverlay
				isOpen={ui.isOverlayOpen}
				isMobileUi={isMobileUi}
				isMenuRoot={ui.isOverlayMenuRoot}
				overlaySection={ui.overlaySection}
				detailSection={overlayDetailSection}
				servers={registry.daemons}
				selectedServerId={ui.selectedServerId}
				onClose={ui.closeOverlay}
				onShowMenuRoot={() => uiState.getState().showOverlayMenuRoot()}
				onSelectSection={(section) =>
					uiState.getState().setOverlaySection(section)
				}
				onConnectServer={(server) => {
					void connectServer(server, { closeOverlay: true });
				}}
				onDeleteServer={(serverId) => {
					void deleteServer(serverId);
				}}
				onOpenAddServer={openAddServerChooser}
			/>

			<ServerModal
				mode={serverModalMode}
				serverForm={serverForm}
				pairingUrl={pairingUrl}
				onClose={closeServerModal}
				onOpenDirect={openDirectServerModal}
				onOpenPairing={openPairingServerModal}
				onSaveServer={saveServer}
				onPairingUrlChange={setPairingUrl}
				onImportPairingLink={importPairingLink}
				onServerFormChange={(updater) => {
					setServerForm((state) => updater(state));
				}}
			/>
		</Shell>
	);
}
