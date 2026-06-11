import { LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
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
import { PairQrCodeModal } from "./PairQrCodeModal";
import {
	initialFormState,
	makeServerRecord,
	type ServerFormState,
	ServerModal,
	type ServerModalMode,
} from "./ServerModal";
import { ServerOptionButtons } from "./ServerOptionButtons";
import { Shell } from "./shell/Shell";
import { useDesktopWindowControls } from "./shell/useDesktopWindowControls";
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
	const [pairQrSvg, setPairQrSvg] = useState<string | null>(null);
	const [isPairUrlCopied, setIsPairUrlCopied] = useState(false);
	const [inlineError, setInlineError] = useState<string | null>(null);
	const [serverScrollbackBytes, setServerScrollbackBytes] = useState<
		number | null
	>(null);
	const [isNarrowViewport, setIsNarrowViewport] = useState(() =>
		typeof window !== "undefined" ? window.innerWidth <= 900 : false,
	);
	const windowControls = useDesktopWindowControls(platform.id === "desktop");
	const {
		services,
		platformError,
		session,
		terminalController,
		terminalRegistry,
		connectServer,
		disconnectCurrentServer,
		copyLocalPairUrl,
		loadLocalPairQrCode,
		restartLocalDaemon,
		importPairingLink: importAndConnectPairingLink,
	} = useAppRuntime({
		platform,
		platformServicesFactory,
		stores,
		onInlineError: setInlineError,
	});
	const pairUrlCopyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
		null,
	);

	const activeServer =
		registry.daemons.find(
			(daemon) => daemon.id === workspace.activeConnectionServerId,
		) ?? null;
	const selectedServer =
		registry.daemons.find((daemon) => daemon.id === ui.selectedServerId) ??
		null;

	useEffect(() => {
		if (session == null || workspace.connectionState !== "connected") {
			setServerScrollbackBytes(null);
			return;
		}

		let cancelled = false;
		void session
			.getServerConfig()
			.then((config) => {
				if (!cancelled) {
					setServerScrollbackBytes(config.scrollback_bytes);
				}
			})
			.catch((error: unknown) => {
				if (!cancelled) {
					setInlineError(
						error instanceof Error
							? error.message
							: "failed to load server scrollback",
					);
				}
			});

		return () => {
			cancelled = true;
		};
	}, [session, workspace.connectionState]);

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

	useEffect(() => {
		return () => {
			if (pairUrlCopyTimerRef.current != null) {
				clearTimeout(pairUrlCopyTimerRef.current);
			}
		};
	}, []);

	const closeServerModal = () => {
		setServerModalMode("closed");
		setInlineError(null);
		setIsPairUrlCopied(false);
	};

	const openAddServerChooser = () => {
		setInlineError(null);
		setPairingUrl("");
		setIsPairUrlCopied(false);
		setServerModalMode("chooser");
	};

	const openDirectServerModal = () => {
		setInlineError(null);
		setServerForm(initialFormState());
		setPairingUrl("");
		setIsPairUrlCopied(false);
		setServerModalMode("direct");
	};

	const openPairingServerModal = () => {
		setInlineError(null);
		setPairingUrl("");
		setIsPairUrlCopied(false);
		setServerModalMode("pairing");
	};

	const openQrScannerModal = () => {
		setInlineError(null);
		setPairingUrl("");
		setIsPairUrlCopied(false);
		setServerModalMode("qr");
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
		await importPairingLinkValue(pairingUrl);
	};

	const importPairingLinkValue = async (rawUrl: string) => {
		try {
			await importAndConnectPairingLink(rawUrl);
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

	const copyPairUrl = async () => {
		const copied = await copyLocalPairUrl();
		if (!copied) {
			return;
		}

		setIsPairUrlCopied(true);
		if (pairUrlCopyTimerRef.current != null) {
			clearTimeout(pairUrlCopyTimerRef.current);
		}
		pairUrlCopyTimerRef.current = setTimeout(() => {
			setIsPairUrlCopied(false);
			pairUrlCopyTimerRef.current = null;
		}, 3000);
	};

	const showPairQrCode = async () => {
		const qrCode = await loadLocalPairQrCode();
		if (qrCode == null) {
			return;
		}
		setPairQrSvg(qrCode.svg);
	};

	const errorMessage = inlineError ?? platformError;
	const hasSavedServers = registry.daemons.length > 0;
	const isMobileOverlay = platform.shell === "mobile" || isNarrowViewport;
	const primaryNavigationMode = ui.isMenuOpen ? "back" : ("menu" as const);
	const handlePrimaryNavigation = () => {
		if (!ui.isMenuOpen) {
			ui.openMenu("settings");
			return;
		}

		if (ui.isMenuRoot) {
			ui.closeMenu();
			return;
		}

		uiState.getState().showMenuRoot();
	};
	const overlayDetailSection =
		ui.menuSection === "settings" ? (
			<HostSettingsSection
				scrollbackBytes={serverScrollbackBytes}
				onScrollbackBytesChange={(scrollbackBytes) => {
					if (session == null) {
						return;
					}
					void session
						.setServerConfig({ scrollback_bytes: scrollbackBytes })
						.then((config) => {
							setServerScrollbackBytes(config.scrollback_bytes);
							setInlineError(null);
						})
						.catch((error: unknown) => {
							setInlineError(
								error instanceof Error
									? error.message
									: "failed to update server scrollback",
							);
						});
				}}
				theme={ui.theme}
				terminalFontSize={ui.terminalFontSize}
				onTerminalFontSizeChange={(fontSize) => {
					ui.setTerminalFontSize(fontSize);
				}}
				onCopyPairUrl={copyPairUrl}
				onShowPairQrCode={showPairQrCode}
				isPairUrlCopied={isPairUrlCopied}
				showPairControls={services?.host != null}
				onRestartLocalDaemon={restartLocalDaemon}
				onThemeChange={(theme) => ui.setTheme(theme)}
			/>
		) : ui.menuSection === "diagnostics" ? (
			<div className="detail-stack">
				<div className="detail-row">
					<span className="detail-row__label">Endpoint</span>
					<div className="detail-row__value">
						<span>{activeServer ? endpointLabel(activeServer) : "none"}</span>
					</div>
				</div>
				<div className="detail-row">
					<span className="detail-row__label">Last error</span>
					<div className="detail-row__value">
						<span>{workspace.lastError ?? "none"}</span>
					</div>
				</div>
			</div>
		) : (
			<div className="detail-stack">
				<div className="detail-row">
					<span className="detail-row__label">Version</span>
					<div className="detail-row__value">
						<span>0.1.0</span>
					</div>
				</div>
			</div>
		);
	const mainContent =
		services == null ? (
			<section
				className="connection-spinner-panel"
				aria-label="Connecting to server"
			>
				<LoaderCircle
					className="connection-spinner"
					aria-hidden="true"
					size={32}
					strokeWidth={1.75}
				/>
				<span className="sr-only">Connecting</span>
			</section>
		) : workspace.connectionState === "connected" ? (
			<TerminalArea
				session={session}
				workspace={workspace}
				isCompactViewport={isNarrowViewport}
				showTerminalControlBar={platform.shell === "mobile"}
				terminalFontSize={ui.terminalFontSize}
				workspaceState={workspaceState}
				controller={terminalController}
				registry={terminalRegistry}
				theme={ui.theme}
				onInlineError={setInlineError}
			/>
		) : workspace.connectionState === "connecting" ? (
			<section
				className="connection-spinner-panel"
				aria-label="Connecting to server"
			>
				<LoaderCircle
					className="connection-spinner"
					aria-hidden="true"
					size={32}
					strokeWidth={1.75}
				/>
				<span className="sr-only">Connecting</span>
			</section>
		) : hasSavedServers ? (
			<section
				className="connection-status-panel"
				aria-label="Connection status"
			>
				<button
					type="button"
					className="retry-button"
					disabled={selectedServer == null}
					onClick={() => {
						if (selectedServer != null) {
							void connectServer(selectedServer);
						}
					}}
				>
					Retry
				</button>
			</section>
		) : (
			<section className="empty-hosts-panel" aria-label="Add server options">
				<div className="server-option-buttons">
					<ServerOptionButtons
						onOpenDirect={openDirectServerModal}
						onOpenPairing={openPairingServerModal}
						onOpenQrScanner={openQrScannerModal}
						showQrScanner={platform.shell === "mobile"}
					/>
				</div>
			</section>
		);

	return (
		<Shell
			activeServerLabel={activeServer?.label ?? null}
			connectionState={workspace.connectionState}
			windowControls={windowControls}
			primaryNavigationMode={primaryNavigationMode}
			onPrimaryNavigation={handlePrimaryNavigation}
		>
			<main className="app-shell__main">
				<div
					className="app-shell__content"
					aria-hidden={ui.isMenuOpen ? "true" : undefined}
				>
					{mainContent}
				</div>
				{ui.isMenuOpen ? (
					<ControlOverlay
						isMobileUi={isMobileOverlay}
						isMenuRoot={ui.isMenuRoot}
						menuSection={ui.menuSection}
						detailSection={overlayDetailSection}
						servers={registry.daemons}
						selectedServerId={ui.selectedServerId}
						onSelectSection={(section) =>
							uiState.getState().setMenuSection(section)
						}
						onConnectServer={(server) => {
							void connectServer(server, { closeMenu: true });
						}}
						onDeleteServer={(serverId) => {
							void deleteServer(serverId);
						}}
						onOpenAddServer={openAddServerChooser}
					/>
				) : null}

				<ErrorBanner message={errorMessage} />
			</main>

			<ServerModal
				mode={serverModalMode}
				serverForm={serverForm}
				pairingUrl={pairingUrl}
				showQrScanner={platform.shell === "mobile"}
				onClose={closeServerModal}
				onOpenDirect={openDirectServerModal}
				onOpenPairing={openPairingServerModal}
				onOpenQrScanner={openQrScannerModal}
				onSaveServer={saveServer}
				onPairingUrlChange={setPairingUrl}
				onImportPairingLink={importPairingLink}
				onImportPairingLinkValue={importPairingLinkValue}
				onPairingQrScannerError={setInlineError}
				onServerFormChange={(updater) => {
					setServerForm((state) => updater(state));
				}}
			/>
			<PairQrCodeModal qrSvg={pairQrSvg} onClose={() => setPairQrSvg(null)} />
		</Shell>
	);
}
