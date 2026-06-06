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
		cols: number;
		rows: number;
		createdAtUnixMs: number;
		attachedClients: number;
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
	upsertTerminal: (terminal: TerminalInfoRecord) => void;
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
	terminalRegistry: TerminalSessionRegistry;
}

interface ConnectionRun {
	id: number;
	serverId: string;
	session: SessionActor | null;
	closed: boolean;
	initialTerminalBootstrapPending: boolean;
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

function errorMessage(error: unknown, fallback: string) {
	return error instanceof Error ? error.message : fallback;
}

export class ConnectionController {
	private activeRun: ConnectionRun | null = null;
	private nextRunId = 0;
	private bootstrapped = false;
	private terminalRefreshTimer: number | null = null;
	private readonly terminalRefreshIntervalMs = 1000;

	constructor(private readonly deps: ControllerDeps) {}

	async bootstrap() {
		if (this.bootstrapped) {
			return;
		}
		this.bootstrapped = true;

		await this.autoConnectSelectedServer();
	}

	async shutdown() {
		const run = this.activeRun;
		this.activeRun = null;
		await this.closeRun(run, { updateWorkspace: false });
	}

	async disconnect() {
		const run = this.activeRun;
		this.activeRun = null;
		await this.closeRun(run, { updateWorkspace: true, reason: null });
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
			this.activeRun?.serverId === server.id &&
			(workspace.connectionState === "connected" ||
				workspace.connectionState === "connecting")
		) {
			return;
		}

		await this.startRun(server.id, daemonRecordToConnectConfig(server));
	}

	getSession() {
		return this.activeRun?.session ?? null;
	}

	private async autoConnectSelectedServer() {
		const registry = this.deps.daemonRegistry.getState();
		const ui = this.deps.uiState.getState();
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

		try {
			await this.startRun(
				selectedServer.id,
				daemonRecordToConnectConfig(selectedServer),
			);
		} catch {
			// startRun has already published the connection failure.
		}
	}

	private async startRun(serverId: string, config: ConnectConfig) {
		const previousRun = this.activeRun;
		const run: ConnectionRun = {
			id: this.nextRunId + 1,
			serverId,
			session: null,
			closed: false,
			initialTerminalBootstrapPending: false,
		};
		this.nextRunId = run.id;
		this.activeRun = run;

		await this.closeRun(previousRun, { updateWorkspace: false });
		if (!this.isCurrent(run)) {
			return;
		}

		this.deps.workspaceState.getState().startConnecting(serverId);
		this.deps.onConnectionReset();
		this.deps.onInlineError(null);

		let session: SessionActor;
		try {
			session = await this.deps.services.sessionFactory.connect(config);
		} catch (error: unknown) {
			if (!this.isCurrent(run)) {
				return;
			}
			this.failRun(run, errorMessage(error, "connection failed"));
			throw error;
		}

		if (!this.isCurrent(run)) {
			await session.close().catch(() => undefined);
			return;
		}

		run.session = session;
		void this.consumeEvents(run);
	}

	private async consumeEvents(run: ConnectionRun) {
		const session = run.session;
		if (session == null) {
			return;
		}

		try {
			for await (const event of session.events()) {
				if (!this.isCurrent(run)) {
					return;
				}
				this.handleEvent(run, event);
			}

			if (this.isCurrent(run)) {
				this.finishRun(run, {
					state: "disconnected",
					willRetry: false,
					reason: "connection closed",
				});
			}
		} catch (error: unknown) {
			if (this.isCurrent(run)) {
				this.failRun(run, errorMessage(error, "event stream failed"));
			}
		}
	}

	private handleEvent(run: ConnectionRun, event: unknown) {
		if (typeof event !== "object" || event === null) {
			return;
		}

		const kind =
			"kind" in event && typeof event.kind === "string" ? event.kind : null;

		switch (kind) {
			case "Connecting":
				return;
			case "Connected":
				this.handleConnected(run, event);
				return;
			case "Disconnected":
				this.handleDisconnected(run, event);
				return;
			case "TerminalList":
				this.handleTerminalList(run, event);
				return;
			case "TerminalCreated":
				this.handleTerminalCreated(event);
				return;
			case "TerminalOutput":
				this.handleTerminalOutput(run, event);
				return;
			case "Error":
				this.failRun(
					run,
					"message" in event && typeof event.message === "string"
						? event.message
						: "runtime error",
				);
				return;
			default:
				return;
		}
	}

	private handleConnected(run: ConnectionRun, event: object) {
		const serverLabel =
			"server_label" in event && typeof event.server_label === "string"
				? event.server_label.trim()
				: "";
		if (serverLabel.length > 0) {
			this.deps.daemonRegistry
				.getState()
				.updateDaemonLabel(run.serverId, serverLabel);
		}

		this.deps.workspaceState.getState().markConnected();
		this.deps.terminalRegistry.connect(run.id);
		run.initialTerminalBootstrapPending = true;
		void this.refreshTerminalsOnce(run);
		this.startTerminalPolling(run);
	}

	private handleDisconnected(run: ConnectionRun, event: object) {
		const reason =
			"reason" in event && typeof event.reason === "string"
				? event.reason
				: "connection closed";
		const willRetry =
			"will_retry" in event && typeof event.will_retry === "boolean"
				? event.will_retry
				: false;

		if (willRetry) {
			this.stopTerminalPolling();
			this.deps.workspaceState
				.getState()
				.markDisconnected({ willRetry: true, reason });
			this.deps.terminalRegistry.disconnect(run.id);
			this.deps.onConnectionReset();
			this.deps.onInlineError(reason);
			return;
		}

		this.finishRun(run, { state: "disconnected", willRetry: false, reason });
	}

	private handleTerminalList(run: ConnectionRun, event: object) {
		const terminals =
			"terminals" in event && Array.isArray(event.terminals)
				? (event.terminals as TerminalInfoRecord[])
				: null;
		if (terminals == null) {
			return;
		}

		this.deps.workspaceState.getState().syncTerminalList(terminals);
		if (!run.initialTerminalBootstrapPending) {
			return;
		}

		run.initialTerminalBootstrapPending = false;
		if (terminals.length === 0) {
			void this.createInitialTerminal(run);
		}
	}

	private handleTerminalCreated(event: object) {
		const info =
			"info" in event && typeof event.info === "object" && event.info !== null
				? event.info
				: null;
		const parsed = parseTerminalInfo(info);
		if (parsed == null) {
			return;
		}

		const workspace = this.deps.workspaceState.getState();
		workspace.upsertTerminal(parsed);
		workspace.setActiveSessionId(parsed.terminal);
	}

	private handleTerminalOutput(run: ConnectionRun, event: object) {
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
		if (terminalId == null || bytesB64 == null || streamSeq == null) {
			return;
		}

		this.deps.terminalRegistry.applyOutput(
			terminalId,
			streamSeq,
			decodeBase64Bytes(bytesB64),
			run.id,
		);
	}

	private finishRun(
		run: ConnectionRun,
		result:
			| { state: "disconnected"; willRetry: false; reason: string | null }
			| { state: "failed"; message: string },
	) {
		if (!this.isCurrent(run)) {
			return;
		}

		run.closed = true;
		this.activeRun = null;
		this.stopTerminalPolling();
		run.session = null;
		this.deps.terminalRegistry.disconnect(run.id);
		this.deps.onConnectionReset();

		if (result.state === "failed") {
			this.deps.workspaceState.getState().markConnectionFailed(result.message);
			this.deps.onInlineError(result.message);
			return;
		}

		this.deps.workspaceState.getState().markDisconnected({
			willRetry: result.willRetry,
			reason: result.reason,
		});
		this.deps.onInlineError(result.reason);
	}

	private failRun(run: ConnectionRun, message: string) {
		this.finishRun(run, { state: "failed", message });
	}

	private async closeRun(
		run: ConnectionRun | null,
		options: { updateWorkspace: boolean; reason?: string | null },
	) {
		if (run == null || run.closed) {
			return;
		}

		run.closed = true;
		this.stopTerminalPolling();
		const session = run.session;
		run.session = null;
		this.deps.terminalRegistry.disconnect(run.id);
		this.deps.onConnectionReset();
		this.deps.onInlineError(options.reason ?? null);
		if (options.updateWorkspace) {
			this.deps.workspaceState
				.getState()
				.markDisconnected({ reason: options.reason ?? null });
		}
		if (session != null) {
			await session.close().catch(() => undefined);
		}
	}

	private isCurrent(run: ConnectionRun) {
		return this.activeRun === run && !run.closed;
	}

	private stopTerminalPolling() {
		if (this.terminalRefreshTimer != null) {
			window.clearTimeout(this.terminalRefreshTimer);
			this.terminalRefreshTimer = null;
		}
	}

	private startTerminalPolling(run: ConnectionRun) {
		this.stopTerminalPolling();

		const tick = async () => {
			if (!this.isCurrent(run) || run.session == null) {
				return;
			}

			await this.refreshTerminalsOnce(run);

			if (!this.isCurrent(run) || run.session == null) {
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

	private async refreshTerminalsOnce(run: ConnectionRun) {
		const session = run.session;
		if (!this.isCurrent(run) || session == null) {
			return;
		}
		const activeRuntimeState = this.deps.terminalRegistry.activeRuntimeState();
		if (
			activeRuntimeState?.phase === "opening" ||
			activeRuntimeState?.phase === "loading_history"
		) {
			return;
		}

		try {
			await session.refreshTerminals();
		} catch {
			// Best-effort polling. Connection lifecycle events handle disconnects.
		}
	}

	private async createInitialTerminal(run: ConnectionRun) {
		const session = run.session;
		if (!this.isCurrent(run) || session == null) {
			return;
		}

		try {
			const createdTerminal = await session.createTerminal({
				cols: 120,
				rows: 36,
			});
			if (this.isCurrent(run) && createdTerminal != null) {
				this.deps.workspaceState
					.getState()
					.setActiveSessionId(createdTerminal.terminal);
			}
		} catch (error: unknown) {
			if (this.isCurrent(run)) {
				this.deps.onInlineError(
					errorMessage(error, "failed to create terminal"),
				);
			}
		}
	}
}
