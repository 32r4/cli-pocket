import { createStore } from "zustand/vanilla";
import type { TerminalSummary } from "@/platform/bridge/types";

export type ConnectionState = "idle" | "connecting" | "connected" | "failed";

interface WorkspaceState {
	connectionState: ConnectionState;
	activeConnectionHostId: string | null;
	terminals: TerminalSummary[];
	activeSessionId: string | null;
	lastError: string | null;
	startConnecting: (hostId: string) => void;
	markConnected: () => void;
	markDisconnected: () => void;
	markConnectionFailed: (message: string) => void;
	openTerminal: (terminal: TerminalSummary) => void;
	markTerminalReady: (terminalId: string) => void;
	markTerminalClosed: (terminalId: string) => void;
	setActiveSessionId: (terminalId: string | null) => void;
	clearError: () => void;
}

export function createWorkspaceStore() {
	return createStore<WorkspaceState>((set) => ({
		connectionState: "idle",
		activeConnectionHostId: null,
		terminals: [],
		activeSessionId: null,
		lastError: null,
		startConnecting: (hostId) =>
			set({
				connectionState: "connecting",
				activeConnectionHostId: hostId,
				activeSessionId: null,
				terminals: [],
				lastError: null,
			}),
		markConnected: () => set({ connectionState: "connected", lastError: null }),
		markDisconnected: () =>
			set({
				connectionState: "idle",
				activeConnectionHostId: null,
				activeSessionId: null,
				terminals: [],
			}),
		markConnectionFailed: (message) =>
			set({
				connectionState: "failed",
				activeConnectionHostId: null,
				activeSessionId: null,
				terminals: [],
				lastError: message,
			}),
		openTerminal: (terminal) =>
			set((state) => ({
				terminals: state.terminals.some((item) => item.id === terminal.id)
					? state.terminals.map((item) =>
							item.id === terminal.id ? terminal : item,
						)
					: [...state.terminals, terminal],
				activeSessionId: terminal.id,
			})),
		markTerminalReady: (terminalId) =>
			set((state) => ({
				terminals: state.terminals.map((terminal) =>
					terminal.id === terminalId
						? { ...terminal, status: "ready" }
						: terminal,
				),
			})),
		markTerminalClosed: (terminalId) =>
			set((state) => {
				const terminals: TerminalSummary[] = state.terminals.map((terminal) =>
					terminal.id === terminalId
						? { ...terminal, status: "closed" }
						: terminal,
				);
				const nextActive =
					state.activeSessionId === terminalId
						? (terminals.find((terminal) => terminal.status !== "closed")?.id ??
							null)
						: state.activeSessionId;

				return {
					terminals,
					activeSessionId: nextActive,
				};
			}),
		setActiveSessionId: (terminalId) => set({ activeSessionId: terminalId }),
		clearError: () => set({ lastError: null }),
	}));
}
