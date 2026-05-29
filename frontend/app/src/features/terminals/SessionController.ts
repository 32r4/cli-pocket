import type { StoreApi } from "zustand/vanilla";
import type {
	ClientBridge,
	ConnectConfig,
	TerminalSummary,
} from "@/platform/bridge/types";

interface WorkspaceState {
	connectionState: "idle" | "connecting" | "connected" | "failed";
	activeConnectionServerId: string | null;
	terminals: TerminalSummary[];
	activeSessionId: string | null;
	lastError: string | null;
	startConnecting: (serverId: string) => void;
	markConnected: () => void;
	markDisconnected: () => void;
	markConnectionFailed: (message: string) => void;
	openTerminal: (terminal: TerminalSummary) => void;
	markTerminalReady: (terminalId: string) => void;
	markTerminalClosed: (terminalId: string) => void;
	setActiveSessionId: (terminalId: string | null) => void;
	clearError: () => void;
}

type WorkspaceStore = StoreApi<WorkspaceState>;

export class SessionController {
	constructor(
		private readonly bridge: ClientBridge,
		private readonly workspace: WorkspaceStore,
	) {}

	async connectAndCreate(serverId: string, config: ConnectConfig) {
		this.workspace.getState().startConnecting(serverId);
		await this.bridge.connect(config);
		this.workspace.getState().markConnected();
		this.workspace.getState().openTerminal({
			id: "pending-terminal",
			title: "shell",
			status: "connecting",
		});
		await this.bridge.createTerminal({ cols: 120, rows: 32 });
		this.workspace.getState().markTerminalReady("pending-terminal");
	}
}
