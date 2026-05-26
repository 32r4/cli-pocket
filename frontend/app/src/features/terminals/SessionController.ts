import type { StoreApi } from "zustand/vanilla";
import type {
	ClientBridge,
	ConnectConfig,
	TerminalSummary,
} from "@/platform/bridge/types";

interface WorkspaceState {
	terminals: TerminalSummary[];
	activeTerminalId: string | null;
	openTerminal: (terminal: TerminalSummary) => void;
	markTerminalReady: (terminalId: string) => void;
}

type WorkspaceStore = StoreApi<WorkspaceState>;

export class SessionController {
	constructor(
		private readonly bridge: ClientBridge,
		private readonly workspace: WorkspaceStore,
	) {}

	async connectAndCreate(config: ConnectConfig) {
		await this.bridge.connect(config);
		this.workspace.getState().openTerminal({
			id: "pending-terminal",
			title: "shell",
			status: "connecting",
		});
		await this.bridge.createTerminal({ cols: 120, rows: 32 });
		this.workspace.getState().markTerminalReady("pending-terminal");
	}
}
