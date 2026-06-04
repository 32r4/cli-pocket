import { createStore } from "zustand/vanilla";
import type {
	TerminalInfoRecord,
	TerminalSummary,
} from "@/platform/bridge/types";

export type ConnectionState = "idle" | "connecting" | "connected" | "failed";

interface WorkspaceState {
	connectionState: ConnectionState;
	activeConnectionServerId: string | null;
	terminals: TerminalSummary[];
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
}

function terminalTitle(info: TerminalInfoRecord, index: number) {
	return info.label?.trim() || `Terminal ${index + 1}`;
}

function toSummary(info: TerminalInfoRecord, index: number): TerminalSummary {
	return {
		id: info.terminal,
		title: terminalTitle(info, index),
		cols: info.cols,
		rows: info.rows,
		createdAtUnixMs: info.created_at_unix_ms,
		attachedClients: info.attached_clients,
	};
}

export function createWorkspaceStore() {
	return createStore<WorkspaceState>((set) => ({
		connectionState: "idle",
		activeConnectionServerId: null,
		terminals: [],
		activeSessionId: null,
		lastError: null,
		startConnecting: (serverId) =>
			set({
				connectionState: "connecting",
				activeConnectionServerId: serverId,
				activeSessionId: null,
				terminals: [],
				lastError: null,
			}),
		markConnected: () => set({ connectionState: "connected", lastError: null }),
		markDisconnected: (options) =>
			set((state) => ({
				connectionState: options?.willRetry ? "connecting" : "idle",
				activeConnectionServerId: options?.willRetry
					? state.activeConnectionServerId
					: null,
				activeSessionId: null,
				terminals: [],
				lastError: options?.reason ?? null,
			})),
		markConnectionFailed: (message) =>
			set({
				connectionState: "failed",
				activeConnectionServerId: null,
				activeSessionId: null,
				terminals: [],
				lastError: message,
			}),
		syncTerminalList: (terminals) =>
			set((state) => {
				const next = [...terminals]
					.sort(
						(left, right) =>
							left.created_at_unix_ms - right.created_at_unix_ms ||
							left.terminal.localeCompare(right.terminal),
					)
					.map((terminal, index) => toSummary(terminal, index));
				const nextActive =
					state.activeSessionId != null &&
					next.some((terminal) => terminal.id === state.activeSessionId)
						? state.activeSessionId
						: state.activeSessionId == null
							? (next[next.length - 1]?.id ?? null)
							: (next[0]?.id ?? null);

				return {
					terminals: next,
					activeSessionId: nextActive,
				};
			}),
		upsertTerminal: (terminal) =>
			set((state) => {
				const existingIndex = state.terminals.findIndex(
					(entry) => entry.id === terminal.terminal,
				);
				if (existingIndex >= 0) {
					return {
						terminals: state.terminals.map((entry, index) =>
							index === existingIndex ? toSummary(terminal, index) : entry,
						),
					};
				}

				return {
					terminals: [
						...state.terminals,
						toSummary(terminal, state.terminals.length),
					],
				};
			}),
		updateTerminalSize: (terminalId, cols, rows) =>
			set((state) => ({
				terminals: state.terminals.map((terminal) =>
					terminal.id === terminalId ? { ...terminal, cols, rows } : terminal,
				),
			})),
		removeTerminal: (terminalId) =>
			set((state) => {
				const terminals = state.terminals.filter(
					(terminal) => terminal.id !== terminalId,
				);
				const activeSessionId =
					state.activeSessionId === terminalId
						? (terminals[0]?.id ?? null)
						: state.activeSessionId;
				return { terminals, activeSessionId };
			}),
		setActiveSessionId: (terminalId) => set({ activeSessionId: terminalId }),
		clearError: () => set({ lastError: null }),
	}));
}
