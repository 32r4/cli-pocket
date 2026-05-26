import { createStore } from "zustand/vanilla";
import type { TerminalSummary } from "@/platform/bridge/types";

interface WorkspaceState {
	terminals: TerminalSummary[];
	activeTerminalId: string | null;
	openTerminal: (terminal: TerminalSummary) => void;
	markTerminalReady: (terminalId: string) => void;
}

export function createWorkspaceStore() {
	return createStore<WorkspaceState>((set) => ({
		terminals: [],
		activeTerminalId: null,
		openTerminal: (terminal) =>
			set((state) => ({
				terminals: [...state.terminals, terminal],
				activeTerminalId: terminal.id,
			})),
		markTerminalReady: (terminalId) =>
			set((state) => ({
				terminals: state.terminals.map((terminal) =>
					terminal.id === terminalId
						? { ...terminal, status: "ready" }
						: terminal,
				),
			})),
	}));
}
