import type { StoreApi } from "zustand/vanilla";
import type { TerminalSessionRegistry } from "@/features/terminals/terminalSessionRegistry";
import type {
	ConnectConfig,
	PlatformServices,
	SessionActor,
	TerminalInfoRecord,
} from "@/platform/bridge/types";
import type { DaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import {
	type DaemonRecord,
	daemonRecordToConnectConfig,
} from "@/state/daemon-registry/types";
import type { MenuSection, ThemeName } from "@/state/ui/uiState";
import type { ConnectionState } from "@/state/workspace/workspaceState";

type UiStateStore = StoreApi<{
	isMenuOpen: boolean;
	menuSection: MenuSection;
	selectedServerId: string | null;
	isMenuRoot: boolean;
	theme: ThemeName;
	openMenu: (section?: MenuSection) => void;
	closeMenu: () => void;
	setMenuSection: (section: MenuSection) => void;
	setSelectedServerId: (serverId: string | null) => void;
	showMenuRoot: () => void;
	setTheme: (theme: ThemeName) => void;
}>;

type WorkspaceStore = StoreApi<{
	connectionState: ConnectionState;
	activeConnectionServerId: string | null;
	terminals: Array<{
		id: string;
		title: string;
		status: "idle" | "connecting" | "ready" | "error";
		cols: number;
		rows: number;
		createdAtUnixMs: number;
		attachedClients: number;
		error: string | null;
	}>;
	activeSessionId: string | null;
	lastError: string | null;
	startConnecting: (serverId: string) => void;
	markConnected: () => void;
	markDisconnected: (options?: {
		willRetry?: boolean;
		reason?: string | null;
	}) => void;
	markConnectionFailed: (message: string) => void;
	syncTerminalList: (terminals: TerminalInfoRecord[]) => void;
	markTerminalConnecting: (terminalId: string) => void;
	markTerminalReady: (info: TerminalInfoRecord) => void;
	markTerminalError: (terminalId: string, message: string) => void;
	updateTerminalSize: (terminalId: string, cols: number, rows: number) => void;
	removeTerminal: (terminalId: string) => void;
	setActiveSessionId: (terminalId: string | null) => void;
	clearError: () => void;
}>;

interface ControllerDeps {
	services: PlatformServices;
	daemonRegistry: DaemonRegistryStore;
	uiState: UiStateStore;
	workspaceState: WorkspaceStore;
	onInlineError: (message: string | null) => void;
	onConnectionReset: () => void;
	onTerminalRemoved: (terminalId: string) => void;
	onConnectionGenerationChange: (generation: number) => void;
	terminalRegistry: TerminalSessionRegistry;
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

export class ConnectionController {
	private session: SessionActor | null = null;
	private connectionGeneration = 0;
	private bootstrapped = false;
	private terminalRefreshTimer: number | null = null;
	private initialTerminalBootstrapPending = false;
	private readonly terminalRefreshIntervalMs = 1000;

	constructor(private readonly deps: ControllerDeps) {}

	private bumpConnectionGeneration() {
		this.connectionGeneration += 1;
		this.deps.onConnectionGenerationChange(this.connectionGeneration);
		return this.connectionGeneration;
	}

	async bootstrap() {
		if (this.bootstrapped) {
			return;
		}
		this.bootstrapped = true;

		await this.autoConnectSelectedServer();
	}

	async shutdown() {
		const generation = this.bumpConnectionGeneration();
		this.stopTerminalPolling();
		const session = this.session;
		this.session = null;
		this.deps.terminalRegistry.disconnect(generation);
		if (session != null) {
			await session.close();
		}
	}

	async disconnect() {
		const generation = this.bumpConnectionGeneration();
		this.stopTerminalPolling();
		const session = this.session;
		this.session = null;
		this.deps.terminalRegistry.disconnect(generation);
		this.deps.workspaceState.getState().markDisconnected({ reason: null });
		this.deps.onConnectionReset();
		this.deps.onInlineError(null);
		if (session != null) {
			await session.close().catch(() => undefined);
		}
	}

	async connectServer(server: DaemonRecord, options?: { closeMenu?: boolean }) {
		this.deps.onInlineError(null);
		this.deps.daemonRegistry.getState().selectDaemon(server.id);
		this.deps.uiState.getState().setSelectedServerId(server.id);
		if (options?.closeMenu === true) {
			this.deps.uiState.getState().closeMenu();
		}

		const workspace = this.deps.workspaceState.getState();
		if (
			workspace.connectionState === "connected" &&
			workspace.activeConnectionServerId === server.id
		) {
			return;
		}

		await this.connect(server.id, daemonRecordToConnectConfig(server));
	}

	private async autoConnectSelectedServer() {
		const registry = this.deps.daemonRegistry.getState();
		const ui = this.deps.uiState.getState();
		const workspace = this.deps.workspaceState.getState();
		const selectedServer =
			registry.daemons.find((daemon) => daemon.id === ui.selectedServerId) ??
			registry.daemons.find(
				(daemon) => daemon.id === registry.selectedDaemonId,
			) ??
			registry.daemons[0] ??
			null;
		if (selectedServer == null) {
			return;
		}
		if (
			workspace.connectionState === "connected" ||
			workspace.connectionState === "connecting"
		) {
			return;
		}

		try {
			await this.connect(
				selectedServer.id,
				daemonRecordToConnectConfig(selectedServer),
			);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "connection failed";
			this.deps.workspaceState.getState().markConnectionFailed(message);
			this.deps.onInlineError(message);
		}
	}

	private async connect(serverId: string, config: ConnectConfig) {
		const generation = this.bumpConnectionGeneration();

		this.stopTerminalPolling();
		const previous = this.session;
		this.session = null;
		if (previous != null) {
			await previous.close();
		}

		this.deps.workspaceState.getState().startConnecting(serverId);
		this.deps.onConnectionReset();
		this.deps.onInlineError(null);

		const session = await this.deps.services.sessionFactory.connect(config);
		if (generation !== this.connectionGeneration) {
			await session.close();
			return;
		}

		this.session = session;
		void this.consumeEvents(session, generation);
	}

	private async consumeEvents(session: SessionActor, generation: number) {
		try {
			for await (const event of session.events()) {
				if (generation !== this.connectionGeneration) {
					return;
				}
				this.handleEvent(event);
			}
		} catch (error: unknown) {
			if (generation !== this.connectionGeneration) {
				return;
			}
			const message =
				error instanceof Error ? error.message : "event stream failed";
			this.deps.workspaceState.getState().markConnectionFailed(message);
			this.deps.onInlineError(message);
		}
	}

	private handleEvent(event: unknown) {
		if (typeof event !== "object" || event === null) {
			return;
		}

		const kind =
			"kind" in event && typeof event.kind === "string" ? event.kind : null;
		if (kind === "Connecting") {
			return;
		}
		if (kind === "Connected") {
			const serverLabel =
				"server_label" in event && typeof event.server_label === "string"
					? event.server_label.trim()
					: "";
			const activeServerId =
				this.deps.workspaceState.getState().activeConnectionServerId;
			if (serverLabel.length > 0 && activeServerId != null) {
				this.deps.daemonRegistry
					.getState()
					.updateDaemonLabel(activeServerId, serverLabel);
			}
			this.deps.workspaceState.getState().markConnected();
			this.initialTerminalBootstrapPending = true;
			void this.refreshTerminalsOnce();
			this.startTerminalPolling();
			return;
		}
		if (kind === "Disconnected") {
			const generation = this.bumpConnectionGeneration();
			const reason =
				"reason" in event && typeof event.reason === "string"
					? event.reason
					: "connection closed";
			const willRetry =
				"will_retry" in event && typeof event.will_retry === "boolean"
					? event.will_retry
					: false;
			this.deps.workspaceState
				.getState()
				.markDisconnected({ willRetry, reason });
			this.deps.terminalRegistry.disconnect(generation);
			this.deps.onConnectionReset();
			this.deps.onInlineError(reason);
			return;
		}
		if (kind === "TerminalList") {
			const terminals =
				"terminals" in event && Array.isArray(event.terminals)
					? (event.terminals as TerminalInfoRecord[])
					: null;
			if (terminals != null) {
				this.deps.workspaceState.getState().syncTerminalList(terminals);
				if (this.initialTerminalBootstrapPending) {
					this.initialTerminalBootstrapPending = false;
					if (terminals.length === 0) {
						void this.createInitialTerminal();
					}
				}
			}
			return;
		}
		if (kind === "TerminalCreated") {
			const info =
				"info" in event && typeof event.info === "object" && event.info !== null
					? event.info
					: null;
			const parsed = parseTerminalInfo(info);
			if (parsed != null) {
				this.deps.workspaceState.getState().markTerminalReady(parsed);
				this.deps.workspaceState.getState().setActiveSessionId(parsed.terminal);
				void this.session?.refreshTerminals();
			}
			return;
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
			const streamSeq =
				"stream_seq" in event && typeof event.stream_seq === "number"
					? event.stream_seq
					: null;
			if (terminalId != null && bytesB64 != null && streamSeq != null) {
				this.deps.terminalRegistry.applyOutput(
					terminalId,
					streamSeq,
					decodeBase64Bytes(bytesB64),
					this.connectionGeneration,
				);
			}
			return;
		}
		if (kind === "TerminalExited") {
			const terminalId =
				"terminal_id" in event && typeof event.terminal_id === "string"
					? event.terminal_id
					: null;
			if (terminalId != null) {
				this.deps.workspaceState.getState().removeTerminal(terminalId);
				this.deps.terminalRegistry.removeTerminal(terminalId);
				this.deps.onTerminalRemoved(terminalId);
				void this.refreshTerminalsOnce();
			}
			return;
		}
		if (kind === "Error") {
			const message =
				"message" in event && typeof event.message === "string"
					? event.message
					: "runtime error";
			this.deps.workspaceState.getState().markConnectionFailed(message);
			this.deps.onInlineError(message);
		}
	}

	getSession() {
		return this.session;
	}

	private stopTerminalPolling() {
		if (this.terminalRefreshTimer != null) {
			window.clearTimeout(this.terminalRefreshTimer);
			this.terminalRefreshTimer = null;
		}
	}

	private startTerminalPolling() {
		this.stopTerminalPolling();
		const generation = this.connectionGeneration;

		const tick = async () => {
			if (generation !== this.connectionGeneration || this.session == null) {
				return;
			}

			await this.refreshTerminalsOnce();

			if (generation !== this.connectionGeneration || this.session == null) {
				return;
			}

			this.terminalRefreshTimer = window.setTimeout(() => {
				this.terminalRefreshTimer = null;
				void tick();
			}, this.terminalRefreshIntervalMs);
		};

		this.terminalRefreshTimer = window.setTimeout(() => {
			this.terminalRefreshTimer = null;
			void tick();
		}, this.terminalRefreshIntervalMs);
	}

	private async refreshTerminalsOnce() {
		const session = this.session;
		if (session == null) {
			return;
		}

		try {
			await session.refreshTerminals();
		} catch {
			// Best-effort polling. Connection lifecycle events handle disconnects.
		}
	}

	private async createInitialTerminal() {
		const session = this.session;
		if (session == null) {
			return;
		}

		try {
			const createdTerminal = await session.createTerminal({
				cols: 120,
				rows: 36,
			});
			if (createdTerminal != null) {
				this.deps.workspaceState
					.getState()
					.setActiveSessionId(createdTerminal.terminal);
			}
		} catch (error: unknown) {
			this.deps.onInlineError(
				error instanceof Error ? error.message : "failed to create terminal",
			);
		}
	}
}
