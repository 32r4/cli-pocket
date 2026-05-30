import { useCallback, useEffect, useRef, useState } from "react";
import { useStore } from "zustand";
import { pairAndStoreDaemon } from "@/features/pairing/pairingFlow";
import { SessionController } from "@/features/terminals/SessionController";
import { XTermView } from "@/features/terminals/XTermView";
import type { ClientBridge, ConnectConfig } from "@/platform/bridge/types";
import {
	type AppPlatform,
	createBridgeForPlatform,
} from "@/platform/runtime/platform";
import { ErrorBanner } from "@/shared/components/ErrorBanner";
import {
	createDaemonRegistryStore,
	emptyPersistedDaemonRegistry,
	type PersistedDaemonRegistry,
} from "@/state/daemon-registry/daemonRegistry";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { createUiStateStore, type OverlaySection } from "@/state/ui/uiState";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { Shell } from "./shell/Shell";

const daemonRegistry = createDaemonRegistryStore();
const uiState = createUiStateStore();
const workspaceState = createWorkspaceStore();

interface AppRootProps {
	platform: AppPlatform;
	bridgeFactory?: (platform: AppPlatform) => Promise<ClientBridge>;
}

type ServerModalMode = "closed" | "chooser" | "direct" | "pairing";

interface ServerFormState {
	kind: "direct" | "relay";
	endpointUrl: string;
	relayUrl: string;
	serverId: string;
	relayPskHex: string;
	serverPublicHex: string;
}

interface ImportPairingOptions {
	clearLocationHash?: boolean;
	closeModal?: boolean;
}

function toConnectConfig(server: DaemonRecord): ConnectConfig {
	if (server.kind === "direct") {
		return {
			kind: "direct",
			endpointUrl: server.endpointUrl,
			resumeTokenHex: server.resumeTokenHex ?? undefined,
		};
	}

	return {
		kind: "relay",
		relayUrl: server.relayUrl,
		serverId: server.serverId,
		pskHex: server.relayPskHex,
		serverPublicHex: server.serverPublicHex,
		resumeTokenHex: server.resumeTokenHex ?? undefined,
	};
}

function serverBadge(server: DaemonRecord) {
	return server.kind === "direct" ? "Local" : "Remote";
}

function endpointLabel(server: DaemonRecord) {
	return server.kind === "direct" ? server.endpointUrl : server.relayUrl;
}

function BackIcon() {
	return <span className="back-button__icon" aria-hidden="true" />;
}

function initialFormState(server?: DaemonRecord): ServerFormState {
	if (server == null) {
		return {
			kind: "direct",
			endpointUrl: "",
			relayUrl: "wss://relay.example/ws/client?server=",
			serverId: "",
			relayPskHex: "",
			serverPublicHex: "",
		};
	}

	if (server.kind === "direct") {
		return {
			kind: "direct",
			endpointUrl: server.endpointUrl,
			relayUrl: "",
			serverId: server.id,
			relayPskHex: "",
			serverPublicHex: "",
		};
	}

	return {
		kind: "relay",
		endpointUrl: "",
		relayUrl: server.relayUrl,
		serverId: server.serverId,
		relayPskHex: server.relayPskHex,
		serverPublicHex: server.serverPublicHex,
	};
}

function currentPairingUrlFromLocation() {
	if (typeof window === "undefined") {
		return null;
	}

	const fragment = window.location.hash.startsWith("#")
		? window.location.hash.slice(1)
		: window.location.hash;
	if (new URLSearchParams(fragment).get("pair") == null) {
		return null;
	}

	return window.location.href;
}

function clearLocationHash() {
	if (typeof window === "undefined" || window.location.hash.length === 0) {
		return;
	}

	const nextUrl =
		window.location.search.length > 0
			? `${window.location.pathname}${window.location.search}`
			: window.location.pathname;
	window.history.replaceState(null, "", nextUrl);
}

function makeServerRecord(
	form: ServerFormState,
	currentServerId: string | null,
): DaemonRecord {
	if (form.kind === "direct") {
		const id = currentServerId ?? crypto.randomUUID();
		return {
			id,
			label: currentServerId ?? id,
			kind: "direct",
			endpointUrl: form.endpointUrl.trim(),
			resumeTokenHex: null,
			lastConnectedAt: null,
		};
	}

	const serverId = form.serverId.trim() || crypto.randomUUID();
	const serverPublicHex = form.serverPublicHex.trim() || "00".repeat(32);
	return {
		id: currentServerId ?? serverId,
		label: currentServerId ?? serverId,
		kind: "relay",
		serverId,
		relayUrl: form.relayUrl.trim(),
		relayPskHex: form.relayPskHex.trim() || "00".repeat(32),
		serverPublicHex,
		resumeTokenHex: null,
		lastConnectedAt: null,
	};
}

export function AppRoot({
	platform,
	bridgeFactory = createBridgeForPlatform,
}: AppRootProps) {
	const registry = useStore(daemonRegistry);
	const ui = useStore(uiState);
	const workspace = useStore(workspaceState);

	const [bridge, setBridge] = useState<ClientBridge | null>(null);
	const [bridgeError, setBridgeError] = useState<string | null>(null);
	const [controller, setController] = useState<SessionController | null>(null);
	const [serverModalMode, setServerModalMode] =
		useState<ServerModalMode>("closed");
	const [serverForm, setServerForm] = useState<ServerFormState>(() =>
		initialFormState(),
	);
	const [pairingUrl, setPairingUrl] = useState("");
	const [inlineError, setInlineError] = useState<string | null>(null);
	const [localPairUrl, setLocalPairUrl] = useState<string | null>(null);
	const [daemonRegistryReady, setDaemonRegistryReady] = useState(false);
	const [eventStreamActive, setEventStreamActive] = useState(false);
	const [pairingImportInProgress, setPairingImportInProgress] = useState(false);
	const [isNarrowViewport, setIsNarrowViewport] = useState(() =>
		typeof window !== "undefined" ? window.innerWidth <= 900 : false,
	);
	const autoConnectServerIdRef = useRef<string | null>(null);
	const pendingPairingServerIdRef = useRef<string | null>(null);
	const pendingInitialTerminalRef = useRef<string | null>(null);
	const processedPairingUrlRef = useRef<string | null>(null);

	const selectedServer =
		registry.daemons.find((daemon) => daemon.id === ui.selectedServerId) ??
		null;
	const activeServer =
		registry.daemons.find(
			(daemon) => daemon.id === workspace.activeConnectionServerId,
		) ?? null;
	const mainServer =
		selectedServer ?? activeServer ?? registry.daemons[0] ?? null;
	const activeSession =
		workspace.terminals.find(
			(terminal) => terminal.id === workspace.activeSessionId,
		) ??
		workspace.terminals[0] ??
		null;
	const hasPendingPairingUrl = currentPairingUrlFromLocation() != null;

	useEffect(() => {
		let active = true;

		void bridgeFactory(platform)
			.then((instance) => {
				if (!active) {
					// The web bridge wraps a wasm global singleton client.
					// Closing a stale instance here tears down the live session
					// created by the StrictMode remount.
					if (platform.bridge !== "web") {
						void instance.close();
					}
					return;
				}

				setBridge(instance);
				setController(new SessionController(instance, workspaceState));
			})
			.catch((error: unknown) => {
				if (!active) {
					return;
				}
				setBridgeError(
					error instanceof Error
						? error.message
						: "failed to start client bridge",
				);
			});

		return () => {
			active = false;
		};
	}, [bridgeFactory, platform]);

	useEffect(() => {
		if (bridge == null) {
			return;
		}

		let cancelled = false;
		setDaemonRegistryReady(false);
		const persistence = {
			load: () => bridge.daemonRegistry.load(),
			save: (state: PersistedDaemonRegistry) =>
				bridge.daemonRegistry.save(state),
		};
		daemonRegistry.setPersistence(persistence);

		void persistence
			.load()
			.then((state) => {
				if (cancelled) {
					return;
				}

				daemonRegistry.hydratePersistedState(
					state ?? emptyPersistedDaemonRegistry(),
				);
				setDaemonRegistryReady(true);
			})
			.catch((error: unknown) => {
				if (cancelled) {
					return;
				}

				daemonRegistry.hydratePersistedState(emptyPersistedDaemonRegistry());
				setInlineError(
					error instanceof Error
						? error.message
						: "failed to restore saved servers",
				);
				setDaemonRegistryReady(true);
			});

		return () => {
			cancelled = true;
		};
	}, [bridge]);

	useEffect(() => {
		if (bridge?.embeddedDaemon == null || !daemonRegistryReady) {
			return;
		}

		let cancelled = false;
		void bridge.embeddedDaemon
			.localEndpoint()
			.then((endpointUrl) => {
				if (cancelled) {
					return;
				}

				const existing = daemonRegistry
					.getState()
					.daemons.find((daemon) => daemon.id === "local-daemon");
				const localDaemon: DaemonRecord = {
					id: "local-daemon",
					label: existing?.label ?? "This desktop",
					kind: "direct",
					endpointUrl,
					resumeTokenHex: existing?.resumeTokenHex ?? null,
					lastConnectedAt: existing?.lastConnectedAt ?? null,
				};
				daemonRegistry.getState().upsertDaemon(localDaemon);
				daemonRegistry.getState().selectDaemon(localDaemon.id);
				uiState.getState().setSelectedServerId(localDaemon.id);
			})
			.catch((error: unknown) => {
				if (cancelled) {
					return;
				}
				setInlineError(
					error instanceof Error
						? error.message
						: "failed to resolve local daemon endpoint",
				);
			});

		return () => {
			cancelled = true;
		};
	}, [bridge, daemonRegistryReady]);

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
		if (!daemonRegistryReady) {
			return;
		}

		const fallbackServerId =
			registry.selectedDaemonId ?? registry.daemons[0]?.id ?? null;
		const selectedServerStillExists = registry.daemons.some(
			(daemon) => daemon.id === ui.selectedServerId,
		);

		if (
			!selectedServerStillExists &&
			ui.selectedServerId !== fallbackServerId
		) {
			uiState.getState().setSelectedServerId(fallbackServerId);
		}

		if (registry.selectedDaemonId == null && fallbackServerId != null) {
			registry.selectDaemon(fallbackServerId);
		}
	}, [
		registry.daemons,
		registry.selectedDaemonId,
		ui.selectedServerId,
		registry,
		daemonRegistryReady,
	]);

	useEffect(() => {
		if (!daemonRegistryReady) {
			return;
		}

		if (mainServer == null) {
			autoConnectServerIdRef.current = null;
			return;
		}
		if (controller == null) {
			return;
		}
		if (hasPendingPairingUrl || pairingImportInProgress) {
			return;
		}
		if (
			workspace.connectionState === "connected" ||
			workspace.connectionState === "connecting"
		) {
			return;
		}
		if (autoConnectServerIdRef.current === mainServer.id) {
			return;
		}

		autoConnectServerIdRef.current = mainServer.id;
		setInlineError(null);

		void controller
			.connect(mainServer.id, toConnectConfig(mainServer))
			.then(() => {
				setEventStreamActive(true);
			})
			.catch((error: unknown) => {
				const message =
					error instanceof Error ? error.message : "connection failed";
				workspaceState.getState().markConnectionFailed(message);
				setInlineError(message);
			});
	}, [
		controller,
		mainServer,
		workspace.connectionState,
		daemonRegistryReady,
		hasPendingPairingUrl,
		pairingImportInProgress,
	]);

	useEffect(() => {
		if (bridge == null || !eventStreamActive) {
			return;
		}

		let cancelled = false;

		void (async () => {
			try {
				for await (const event of bridge.events()) {
					if (cancelled || typeof event !== "object" || event === null) {
						if (cancelled) {
							break;
						}
						continue;
					}

					const kind =
						"kind" in event && typeof event.kind === "string"
							? event.kind
							: null;
					if (kind === "Connecting") {
						continue;
					}
					if (kind === "Connected") {
						const serverLabel =
							"server_label" in event && typeof event.server_label === "string"
								? event.server_label.trim()
								: "";
						if (
							serverLabel.length > 0 &&
							workspaceState.getState().activeConnectionServerId != null
						) {
							daemonRegistry
								.getState()
								.updateDaemonLabel(
									workspaceState.getState().activeConnectionServerId as string,
									serverLabel,
								);
						}
						workspaceState.getState().markConnected();
						const activeServerId =
							workspaceState.getState().activeConnectionServerId;
						if (
							activeServerId != null &&
							workspaceState.getState().terminals.length === 0 &&
							pendingInitialTerminalRef.current !== activeServerId
						) {
							pendingInitialTerminalRef.current = activeServerId;
							void bridge
								.createTerminal({ cols: 120, rows: 32 })
								.catch((error: unknown) => {
									pendingInitialTerminalRef.current = null;
									const message =
										error instanceof Error
											? error.message
											: "failed to create terminal";
									workspaceState.getState().markConnectionFailed(message);
									setInlineError(message);
								});
						}
						continue;
					}
					if (kind === "Disconnected") {
						pendingInitialTerminalRef.current = null;
						workspaceState.getState().markDisconnected();
						continue;
					}
					if (kind === "TerminalCreated") {
						const info =
							"info" in event &&
							typeof event.info === "object" &&
							event.info !== null
								? event.info
								: null;
						const terminalId =
							info != null &&
							"terminal" in info &&
							typeof info.terminal === "string"
								? info.terminal
								: `terminal-${workspaceState.getState().terminals.length + 1}`;
						const label =
							info != null && "label" in info && typeof info.label === "string"
								? info.label
								: `Terminal ${workspaceState.getState().terminals.length + 1}`;

						workspaceState.getState().openTerminal({
							id: terminalId,
							title: label,
							status: "ready",
						});
						pendingInitialTerminalRef.current = null;
						continue;
					}
					if (kind === "TerminalExited") {
						const terminalId =
							"terminal_id" in event && typeof event.terminal_id === "string"
								? event.terminal_id
								: null;
						if (terminalId !== null) {
							workspaceState.getState().markTerminalClosed(terminalId);
						}
						continue;
					}
					if (kind === "Error") {
						const message =
							"message" in event && typeof event.message === "string"
								? event.message
								: "runtime error";
						pendingInitialTerminalRef.current = null;
						workspaceState.getState().markConnectionFailed(message);
						setInlineError(message);
					}
				}
			} catch (error: unknown) {
				if (cancelled) {
					return;
				}
				const message =
					error instanceof Error ? error.message : "event stream failed";
				pendingInitialTerminalRef.current = null;
				workspaceState.getState().markConnectionFailed(message);
				setInlineError(message);
			}
		})();

		return () => {
			cancelled = true;
		};
	}, [bridge, eventStreamActive]);

	useEffect(() => {
		const pendingServerId = pendingPairingServerIdRef.current;
		if (pendingServerId == null) {
			return;
		}

		if (
			workspace.connectionState === "connected" &&
			workspace.activeConnectionServerId === pendingServerId
		) {
			pendingPairingServerIdRef.current = null;
		}

		if (workspace.connectionState === "failed") {
			pendingPairingServerIdRef.current = null;
		}
	}, [workspace.activeConnectionServerId, workspace.connectionState]);

	const closeServerModal = useCallback(() => {
		setServerModalMode("closed");
		setInlineError(null);
	}, []);

	const importPairingUrl = useCallback(
		async (rawUrl: string, options?: ImportPairingOptions) => {
			try {
				if (controller == null) {
					throw new Error("client bridge not ready");
				}

				setPairingImportInProgress(true);
				const importedServer = await pairAndStoreDaemon(
					rawUrl,
					async (serverId, config) => {
						pendingPairingServerIdRef.current = serverId;
						await controller.connect(serverId, config);
						setEventStreamActive(true);
						await new Promise<void>((resolve, reject) => {
							const startedAt = Date.now();
							const interval = window.setInterval(() => {
								const state = workspaceState.getState();
								if (
									state.connectionState === "connected" &&
									state.activeConnectionServerId === serverId
								) {
									window.clearInterval(interval);
									resolve();
									return;
								}
								if (state.connectionState === "failed") {
									window.clearInterval(interval);
									reject(
										new Error(
											state.lastError ?? "failed to connect paired server",
										),
									);
									return;
								}
								if (Date.now() - startedAt > 15_000) {
									window.clearInterval(interval);
									reject(new Error("timed out waiting for paired connection"));
								}
							}, 100);
						});
					},
					registry.upsertDaemon,
				);
				registry.selectDaemon(importedServer.id);
				uiState.getState().setSelectedServerId(importedServer.id);
				setPairingUrl("");
				if (options?.closeModal === true) {
					closeServerModal();
				}
			} catch (error: unknown) {
				setInlineError(
					error instanceof Error
						? error.message
						: "failed to import pairing link",
				);
			} finally {
				setPairingImportInProgress(false);
				if (options?.clearLocationHash === true) {
					clearLocationHash();
				}
			}
		},
		[closeServerModal, controller, registry],
	);

	useEffect(() => {
		if (!daemonRegistryReady || controller == null) {
			return;
		}

		const rawUrl = currentPairingUrlFromLocation();
		if (rawUrl == null || processedPairingUrlRef.current === rawUrl) {
			return;
		}

		processedPairingUrlRef.current = rawUrl;
		setInlineError(null);
		void importPairingUrl(rawUrl, { clearLocationHash: true });
	}, [controller, daemonRegistryReady, importPairingUrl]);

	const connectServer = async (
		server: DaemonRecord,
		options?: { closeOverlay?: boolean },
	) => {
		setInlineError(null);
		autoConnectServerIdRef.current = server.id;
		registry.selectDaemon(server.id);
		uiState.getState().setSelectedServerId(server.id);
		if (options?.closeOverlay === true) {
			uiState.getState().closeOverlay();
		}
		if (
			workspaceState.getState().connectionState === "connected" &&
			workspaceState.getState().activeConnectionServerId === server.id
		) {
			return;
		}
		if (controller == null) {
			return;
		}

		try {
			await controller.connect(server.id, toConnectConfig(server));
			setEventStreamActive(true);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "connection failed";
			workspaceState.getState().markConnectionFailed(message);
			setInlineError(message);
		}
	};

	const createSession = async () => {
		if (bridge == null || workspace.connectionState !== "connected") {
			return;
		}

		const nextIndex = workspace.terminals.length + 1;
		const tempId = `pending-terminal-${nextIndex}`;
		workspaceState.getState().openTerminal({
			id: tempId,
			title: `Terminal ${nextIndex}`,
			status: "connecting",
		});

		try {
			await bridge.createTerminal({ cols: 120, rows: 36 });
			workspaceState.getState().markTerminalReady(tempId);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to create terminal";
			workspaceState.getState().markTerminalClosed(tempId);
			setInlineError(message);
		}
	};

	const generateLocalPairUrl = () => {
		if (bridge?.embeddedDaemon == null) {
			return;
		}

		void bridge.embeddedDaemon
			.pairUrl()
			.then((url) => {
				setLocalPairUrl(url);
				setInlineError(null);
			})
			.catch((error: unknown) => {
				setInlineError(
					error instanceof Error
						? error.message
						: "failed to generate pair url",
				);
			});
	};

	const restartLocalDaemon = () => {
		if (bridge?.embeddedDaemon == null) {
			return;
		}

		void bridge.embeddedDaemon
			.restart()
			.then(() => {
				setInlineError(null);
			})
			.catch((error: unknown) => {
				setInlineError(
					error instanceof Error ? error.message : "failed to restart daemon",
				);
			});
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
		const nextServer = makeServerRecord(serverForm, null);
		registry.upsertDaemon(nextServer);
		registry.selectDaemon(nextServer.id);
		uiState.getState().setSelectedServerId(nextServer.id);
		closeServerModal();
	};

	const importPairingLink = async () => {
		await importPairingUrl(pairingUrl, { closeModal: true });
	};

	const errorMessage = inlineError ?? workspace.lastError ?? bridgeError;
	const hasSavedServers = daemonRegistryReady && registry.daemons.length > 0;
	const isMobileUi = platform.shell === "mobile" || isNarrowViewport;
	const mobileOverlayShowsDetail = isMobileUi && !ui.isOverlayMenuRoot;
	const overlayDetailSection =
		ui.overlaySection === "settings" ? (
			<section className="detail-section">
				<h2>Settings</h2>
				{bridge?.embeddedDaemon != null ? (
					<div className="action-row">
						<button type="button" onClick={generateLocalPairUrl}>
							Generate pair URL
						</button>
						<button type="button" onClick={restartLocalDaemon}>
							Restart daemon
						</button>
					</div>
				) : null}
				{bridge?.embeddedDaemon != null && localPairUrl != null ? (
					<div className="detail-grid">
						<div>
							<span>Pair URL</span>
							<strong>{localPairUrl}</strong>
						</div>
					</div>
				) : null}
				<div className="detail-grid">
					<div>
						<span>Theme</span>
						<strong>Dark</strong>
					</div>
					<div>
						<span>Shell</span>
						<strong>default</strong>
					</div>
					<div>
						<span>Scrollback</span>
						<strong>4194304</strong>
					</div>
					<div>
						<span>Keyboard</span>
						<strong>virtual key bar on touch input</strong>
					</div>
				</div>
			</section>
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
				{!daemonRegistryReady ? (
					<section
						className="connection-status-panel"
						aria-label="Restoring saved servers"
					>
						<h2>Loading servers</h2>
						<p>Restoring saved servers.</p>
					</section>
				) : workspace.connectionState === "connected" ? (
					<section className="workspace-panel" aria-label="Terminal workspace">
						<div className="terminal-tabs" role="tablist" aria-label="Sessions">
							{workspace.terminals.map((terminal) => (
								<button
									className="terminal-tab"
									type="button"
									key={terminal.id}
									data-active={terminal.id === activeSession?.id}
									onClick={() =>
										workspaceState.getState().setActiveSessionId(terminal.id)
									}
								>
									{terminal.title}
								</button>
							))}
							<button
								className="terminal-tab terminal-tab--add"
								type="button"
								onClick={() => void createSession()}
							>
								+
							</button>
						</div>
						<div className="terminal-stage">
							<XTermView title={activeSession?.title ?? "Terminal"} />
						</div>
						<footer className="terminal-footer">
							<span>{activeSession?.title ?? "No terminal"}</span>
							<span>120x36</span>
							<span>{activeSession?.status ?? workspace.connectionState}</span>
						</footer>
					</section>
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

			{ui.isOverlayOpen && isMobileUi ? (
				<aside
					className="control-overlay control-overlay--mobile"
					aria-label="Control overlay"
				>
					{mobileOverlayShowsDetail ? (
						<div className="control-overlay__mobile-page">
							<button
								type="button"
								className="back-button"
								onClick={() => uiState.getState().showOverlayMenuRoot()}
								aria-label="Back to menu"
							>
								<BackIcon />
							</button>
							{overlayDetailSection}
						</div>
					) : (
						<div className="control-overlay__mobile-page">
							<button
								type="button"
								className="back-button"
								onClick={ui.closeOverlay}
								aria-label="Close menu"
							>
								<BackIcon />
							</button>
							<nav className="overlay-nav" aria-label="Overlay sections">
								{(["settings", "diagnostics", "about"] as OverlaySection[]).map(
									(section) => (
										<button
											type="button"
											key={section}
											onClick={() =>
												uiState.getState().setOverlaySection(section)
											}
										>
											{section.charAt(0).toUpperCase() + section.slice(1)}
										</button>
									),
								)}
							</nav>
							<div className="overlay-divider" aria-hidden="true" />
							<div className="server-list">
								<p className="server-list__heading">Saved servers</p>
								{registry.daemons.map((server) => (
									<button
										type="button"
										key={server.id}
										className="server-list__item"
										onClick={() =>
											void connectServer(server, { closeOverlay: true })
										}
									>
										<span>{server.label}</span>
										<small>{serverBadge(server)}</small>
									</button>
								))}
								<button
									type="button"
									className="server-list__add"
									onClick={openAddServerChooser}
								>
									+ Add server
								</button>
							</div>
						</div>
					)}
				</aside>
			) : null}

			{ui.isOverlayOpen && !isMobileUi ? (
				<aside className="control-overlay" aria-label="Control overlay">
					<div className="control-overlay__rail">
						<button
							type="button"
							className="back-button"
							onClick={ui.closeOverlay}
							aria-label="Close menu"
						>
							<BackIcon />
						</button>
						<nav className="overlay-nav" aria-label="Overlay sections">
							{(["settings", "diagnostics", "about"] as OverlaySection[]).map(
								(section) => (
									<button
										type="button"
										key={section}
										data-active={ui.overlaySection === section}
										onClick={() =>
											uiState.getState().setOverlaySection(section)
										}
									>
										{section.charAt(0).toUpperCase() + section.slice(1)}
									</button>
								),
							)}
						</nav>
						<div className="overlay-divider" aria-hidden="true" />
						<div className="server-list">
							<p className="server-list__heading">Saved servers</p>
							{registry.daemons.map((server) => (
								<button
									type="button"
									key={server.id}
									className="server-list__item"
									data-active={ui.selectedServerId === server.id}
									onClick={() =>
										void connectServer(server, { closeOverlay: true })
									}
								>
									<span>{server.label}</span>
									<small>{serverBadge(server)}</small>
								</button>
							))}
							<button
								type="button"
								className="server-list__add"
								onClick={openAddServerChooser}
							>
								+ Add server
							</button>
						</div>
					</div>
					<div className="control-overlay__detail">{overlayDetailSection}</div>
				</aside>
			) : null}

			{serverModalMode !== "closed" ? (
				<div className="server-modal-backdrop">
					<div
						className="server-modal"
						role="dialog"
						aria-modal="true"
						aria-label="Add server modal"
					>
						<h2>
							{serverModalMode === "chooser"
								? "Add server"
								: serverModalMode === "direct"
									? "Direct connection"
									: "Pairing link"}
						</h2>

						{serverModalMode === "chooser" ? (
							<div className="server-option-list">
								<button type="button" onClick={openDirectServerModal}>
									Direct connection
								</button>
								<button type="button" onClick={openPairingServerModal}>
									Pairing link
								</button>
								<button type="button" disabled>
									QR code
								</button>
							</div>
						) : null}

						{serverModalMode === "direct" ? (
							<form
								className="server-form"
								onSubmit={(event) => {
									event.preventDefault();
									saveServer();
								}}
							>
								<label className="field">
									<span>Endpoint URL</span>
									<input
										value={serverForm.endpointUrl}
										onChange={(event) =>
											setServerForm((state) => ({
												...state,
												kind: "direct",
												endpointUrl: event.target.value,
											}))
										}
									/>
								</label>
								<div className="action-row">
									<button type="submit">Save server</button>
									<button type="button" onClick={closeServerModal}>
										Cancel
									</button>
								</div>
							</form>
						) : null}

						{serverModalMode === "pairing" ? (
							<div className="server-form">
								<label className="field">
									<span>Pairing link</span>
									<input
										value={pairingUrl}
										onChange={(event) => setPairingUrl(event.target.value)}
										placeholder="https://cli-pocket...#pair=..."
									/>
								</label>
								<div className="action-row">
									<button
										type="button"
										onClick={() => void importPairingLink()}
									>
										Import
									</button>
									<button type="button" onClick={closeServerModal}>
										Cancel
									</button>
								</div>
							</div>
						) : null}
					</div>
				</div>
			) : null}
		</Shell>
	);
}
