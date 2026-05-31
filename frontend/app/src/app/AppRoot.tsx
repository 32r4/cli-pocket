import { useCallback, useEffect, useRef, useState } from "react";
import { useStore } from "zustand";
import { importPairingOfferUrl } from "@/features/pairing/pairingOffer";
import { TerminalViewport } from "@/features/terminals/TerminalViewport";
import { TerminalController } from "@/features/terminals/terminalController";
import type {
	ClientBridge,
	ConnectConfig,
	TerminalInfoRecord,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
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
import {
	type DaemonRecord,
	daemonRecordToConnectConfig,
} from "@/state/daemon-registry/types";
import {
	createUiStateStore,
	type OverlaySection,
	type ThemeName,
} from "@/state/ui/uiState";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { Shell } from "./shell/Shell";

const daemonRegistry = createDaemonRegistryStore();
const uiState = createUiStateStore();
const workspaceState = createWorkspaceStore();
const TERMINAL_LIST_POLL_MS = 2_000;

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

function serverBadge(server: DaemonRecord) {
	return server.kind === "direct" ? "Local" : "Remote";
}

function endpointLabel(server: DaemonRecord) {
	return server.kind === "direct" ? server.endpointUrl : server.relayUrl;
}

function themeLabel(theme: ThemeName) {
	return theme === "light" ? "Light" : "Dark";
}

function parseTerminalInfo(value: unknown): TerminalInfoRecord | null {
	if (typeof value !== "object" || value === null) {
		return null;
	}

	const terminal =
		"terminal" in value && typeof value.terminal === "string"
			? value.terminal
			: null;
	if (terminal == null) {
		return null;
	}

	return {
		terminal,
		cols: "cols" in value && typeof value.cols === "number" ? value.cols : 120,
		rows: "rows" in value && typeof value.rows === "number" ? value.rows : 32,
		created_at_unix_ms:
			"created_at_unix_ms" in value &&
			typeof value.created_at_unix_ms === "number"
				? value.created_at_unix_ms
				: 0,
		label:
			"label" in value && typeof value.label === "string" ? value.label : null,
		attached_clients:
			"attached_clients" in value && typeof value.attached_clients === "number"
				? value.attached_clients
				: 0,
	};
}

function decodeBase64Bytes(value: string) {
	const binary = window.atob(value);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return new TextDecoder().decode(bytes);
}

function parseTerminalSnapshot(value: unknown): TerminalSnapshotRecord | null {
	if (typeof value !== "object" || value === null) {
		return null;
	}

	const info =
		"info" in value && typeof value.info === "object" && value.info !== null
			? parseTerminalInfo(value.info)
			: null;
	const snapshotBytes =
		"snapshot_bytes_b64" in value &&
		typeof value.snapshot_bytes_b64 === "string"
			? value.snapshot_bytes_b64
			: null;
	if (info == null || snapshotBytes == null) {
		return null;
	}

	return {
		info,
		snapshot_bytes_b64: snapshotBytes,
	};
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
	const [serverModalMode, setServerModalMode] =
		useState<ServerModalMode>("closed");
	const [serverForm, setServerForm] = useState<ServerFormState>(() =>
		initialFormState(),
	);
	const [pairingUrl, setPairingUrl] = useState("");
	const [inlineError, setInlineError] = useState<string | null>(null);
	const [localPairUrl, setLocalPairUrl] = useState<string | null>(null);
	const [daemonRegistryReady, setDaemonRegistryReady] = useState(false);
	const [pairingImportInProgress, setPairingImportInProgress] = useState(false);
	const [eventStreamStarted, setEventStreamStarted] = useState(false);
	const [isNarrowViewport, setIsNarrowViewport] = useState(() =>
		typeof window !== "undefined" ? window.innerWidth <= 900 : false,
	);
	const autoConnectServerIdRef = useRef<string | null>(null);
	const pendingPairingServerIdRef = useRef<string | null>(null);
	const processedPairingUrlRef = useRef<string | null>(null);
	const terminalControllerRef = useRef<TerminalController | null>(null);

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
		) ?? null;
	const hasPendingPairingUrl = currentPairingUrlFromLocation() != null;
	const startEventStream = useCallback(() => {
		setEventStreamStarted(true);
	}, []);
	const connectBridge = useCallback(
		async (serverId: string, config: ConnectConfig) => {
			if (bridge == null) {
				throw new Error("client bridge not ready");
			}

			workspaceState.getState().startConnecting(serverId);
			terminalControllerRef.current?.reset();
			await bridge.connect(config);
		},
		[bridge],
	);

	if (terminalControllerRef.current == null) {
		terminalControllerRef.current = new TerminalController({
			onInput: (terminalId, data) => {
				void sendInputToTerminal(terminalId, data);
			},
			onResize: (terminalId, cols, rows) => {
				void resizeTerminalById(terminalId, cols, rows);
			},
		});
	} else {
		terminalControllerRef.current.setHandlers({
			onInput: (terminalId, data) => {
				void sendInputToTerminal(terminalId, data);
			},
			onResize: (terminalId, cols, rows) => {
				void resizeTerminalById(terminalId, cols, rows);
			},
		});
	}
	const terminalController = terminalControllerRef.current;

	useEffect(() => {
		let active = true;
		let activeInstance: ClientBridge | null = null;

		setEventStreamStarted(false);

		void bridgeFactory(platform)
			.then((instance) => {
				if (!active) {
					return;
				}

				activeInstance = instance;
				setBridge(instance);
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
			if (activeInstance != null) {
				void activeInstance.close();
			}
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
		if (bridge == null) {
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

		void connectBridge(mainServer.id, daemonRecordToConnectConfig(mainServer))
			.then(() => {
				startEventStream();
			})
			.catch((error: unknown) => {
				const message =
					error instanceof Error ? error.message : "connection failed";
				workspaceState.getState().markConnectionFailed(message);
				setInlineError(message);
			});
	}, [
		mainServer,
		workspace.connectionState,
		bridge,
		daemonRegistryReady,
		hasPendingPairingUrl,
		pairingImportInProgress,
		connectBridge,
		startEventStream,
	]);

	useEffect(() => {
		if (bridge == null || !eventStreamStarted) {
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
						continue;
					}
					if (kind === "Disconnected") {
						const reason =
							"reason" in event && typeof event.reason === "string"
								? event.reason
								: "connection closed";
						workspaceState.getState().markDisconnected();
						terminalControllerRef.current?.reset();
						setInlineError(reason);
						continue;
					}
					if (kind === "TerminalCreated") {
						const info =
							"info" in event &&
							typeof event.info === "object" &&
							event.info !== null
								? event.info
								: null;
						const parsed = parseTerminalInfo(info);
						if (parsed != null) {
							workspaceState.getState().markTerminalReady(parsed);
							workspaceState.getState().setActiveSessionId(parsed.terminal);
						}
						continue;
					}
					if (kind === "TerminalOutput") {
						const terminalId =
							"terminal_id" in event && typeof event.terminal_id === "string"
								? event.terminal_id
								: null;
						const bytesB64 =
							"bytes_b64" in event && typeof event.bytes_b64 === "string"
								? event.bytes_b64
								: null;
						if (terminalId !== null && bytesB64 !== null) {
							const chunk = decodeBase64Bytes(bytesB64);
							terminalControllerRef.current?.appendActiveOutput(
								terminalId,
								chunk,
							);
						}
						continue;
					}
					if (kind === "TerminalExited") {
						const terminalId =
							"terminal_id" in event && typeof event.terminal_id === "string"
								? event.terminal_id
								: null;
						if (terminalId !== null) {
							workspaceState.getState().removeTerminal(terminalId);
							terminalControllerRef.current?.removeTerminal(terminalId);
						}
						continue;
					}
					if (kind === "Error") {
						const message =
							"message" in event && typeof event.message === "string"
								? event.message
								: "runtime error";
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
				workspaceState.getState().markConnectionFailed(message);
				setInlineError(message);
			}
		})();

		return () => {
			cancelled = true;
		};
	}, [bridge, eventStreamStarted]);

	useEffect(() => {
		if (
			bridge == null ||
			workspace.connectionState !== "connected" ||
			!eventStreamStarted
		) {
			return;
		}

		let cancelled = false;
		const poll = async () => {
			try {
				const terminals = await bridge.listTerminals();
				if (!cancelled) {
					workspaceState.getState().syncTerminalList(terminals);
				}
			} catch (error: unknown) {
				if (!cancelled) {
					setInlineError(
						error instanceof Error ? error.message : "failed to list terminals",
					);
				}
			}
		};

		void poll();
		const interval = window.setInterval(
			() => void poll(),
			TERMINAL_LIST_POLL_MS,
		);

		return () => {
			cancelled = true;
			window.clearInterval(interval);
		};
	}, [bridge, eventStreamStarted, workspace.connectionState]);

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
				setPairingImportInProgress(true);
				const importedServer = importPairingOfferUrl(rawUrl);
				pendingPairingServerIdRef.current = importedServer.id;
				await connectBridge(
					importedServer.id,
					daemonRecordToConnectConfig(importedServer),
				);
				startEventStream();
				await new Promise<void>((resolve, reject) => {
					const startedAt = Date.now();
					const interval = window.setInterval(() => {
						const state = workspaceState.getState();
						if (
							state.connectionState === "connected" &&
							state.activeConnectionServerId === importedServer.id
						) {
							window.clearInterval(interval);
							resolve();
							return;
						}
						if (state.connectionState === "failed") {
							window.clearInterval(interval);
							reject(
								new Error(state.lastError ?? "failed to connect paired server"),
							);
							return;
						}
						if (Date.now() - startedAt > 15_000) {
							window.clearInterval(interval);
							reject(new Error("timed out waiting for paired connection"));
						}
					}, 100);
				});
				registry.upsertDaemon(importedServer);
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
		[closeServerModal, connectBridge, registry, startEventStream],
	);

	useEffect(() => {
		if (!daemonRegistryReady || bridge == null) {
			return;
		}

		const rawUrl = currentPairingUrlFromLocation();
		if (rawUrl == null || processedPairingUrlRef.current === rawUrl) {
			return;
		}

		processedPairingUrlRef.current = rawUrl;
		setInlineError(null);
		void importPairingUrl(rawUrl, { clearLocationHash: true });
	}, [bridge, daemonRegistryReady, importPairingUrl]);

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
		if (bridge == null) {
			return;
		}

		try {
			await connectBridge(server.id, daemonRecordToConnectConfig(server));
			startEventStream();
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

		try {
			await bridge.createTerminal({ cols: 120, rows: 36 });
			const terminals = await bridge.listTerminals();
			workspaceState.getState().syncTerminalList(terminals);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to create terminal";
			setInlineError(message);
		}
	};

	const selectTerminal = async (terminalId: string) => {
		workspaceState.getState().setActiveSessionId(terminalId);
		terminalControllerRef.current?.setActiveTerminal(terminalId);
		if (bridge == null) {
			return;
		}

		workspaceState.getState().markTerminalConnecting(terminalId);
		try {
			const snapshot = await Promise.race([
				bridge.openTerminal(terminalId),
				new Promise<never>((_, reject) => {
					window.setTimeout(
						() => reject(new Error("terminal open timed out")),
						5_000,
					);
				}),
			]);
			const parsed = parseTerminalSnapshot(snapshot);
			if (parsed == null) {
				throw new Error("invalid terminal snapshot");
			}
			workspaceState.getState().markTerminalReady(parsed.info);
			terminalControllerRef.current?.renderSnapshot(
				terminalId,
				decodeBase64Bytes(parsed.snapshot_bytes_b64),
			);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to open terminal";
			workspaceState.getState().markTerminalError(terminalId, message);
			setInlineError(message);
		}
	};

	const killTerminal = async (terminalId: string) => {
		if (bridge == null) {
			return;
		}

		try {
			await bridge.kill(terminalId, "TERM");
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to kill terminal";
			setInlineError(message);
		}
	};
	const sendInputToTerminal = async (terminalId: string, data: string) => {
		if (bridge == null) {
			return;
		}

		try {
			await bridge.sendInput(terminalId, new TextEncoder().encode(data));
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to send input";
			setInlineError(message);
		}
	};

	const resizeTerminalById = async (
		terminalId: string,
		cols: number,
		rows: number,
	) => {
		if (bridge == null) {
			return;
		}

		workspaceState.getState().updateTerminalSize(terminalId, cols, rows);
		try {
			await bridge.resize(terminalId, cols, rows);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to resize terminal";
			setInlineError(message);
		}
	};

	useEffect(() => {
		terminalControllerRef.current?.setTheme(ui.theme);
	}, [ui.theme]);

	useEffect(() => {
		terminalControllerRef.current?.setActiveTerminal(activeSession?.id ?? null);
	}, [activeSession?.id]);

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
				<fieldset className="field-stack theme-fieldset">
					<legend className="sr-only">Theme preference</legend>
					<div className="action-row">
						<button
							type="button"
							data-active={ui.theme === "dark"}
							className="theme-toggle"
							aria-label="Use dark theme"
							onClick={() => ui.setTheme("dark")}
						>
							Dark
						</button>
						<button
							type="button"
							data-active={ui.theme === "light"}
							className="theme-toggle"
							aria-label="Use light theme"
							onClick={() => ui.setTheme("light")}
						>
							Light
						</button>
					</div>
				</fieldset>
				<div className="detail-grid">
					<div>
						<span>Theme</span>
						<strong>{themeLabel(ui.theme)}</strong>
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
								<div
									className="terminal-tab"
									key={terminal.id}
									data-active={terminal.id === activeSession?.id}
								>
									<button
										className="terminal-tab__button"
										type="button"
										onClick={() => void selectTerminal(terminal.id)}
									>
										<span className="terminal-tab__label">
											{terminal.title}
										</span>
									</button>
									<button
										className="terminal-tab__close"
										type="button"
										aria-label={`Kill ${terminal.title}`}
										onClick={(event) => {
											event.stopPropagation();
											void killTerminal(terminal.id);
										}}
									>
										x
									</button>
								</div>
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
							{activeSession == null ? (
								<div className="xterm-server">Select a terminal</div>
							) : activeSession.status === "connecting" ? (
								<div className="xterm-server">Connecting terminal...</div>
							) : activeSession.status === "error" ? (
								<div className="xterm-server">
									{activeSession.error ?? "Terminal attach failed"}
								</div>
							) : (
								<TerminalViewport controller={terminalController} />
							)}
						</div>
						<footer className="terminal-footer">
							<span>{activeSession?.title ?? "No terminal"}</span>
							<span>
								{activeSession == null
									? "--"
									: `${activeSession.cols}x${activeSession.rows}`}
							</span>
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
