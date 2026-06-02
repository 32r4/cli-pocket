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
	markTerminalConnecting: (terminalId: string) => void;
	markTerminalReady: (info: TerminalInfoRecord) => void;
	markTerminalError: (terminalId: string, message: string) => void;
	updateTerminalSize: (terminalId: string, cols: number, rows: number) => void;
	removeTerminal: (terminalId: string) => void;
	setActiveSessionId: (terminalId: string | null) => void;
	clearError: () => void;
}

function terminalTitle(info: TerminalInfoRecord, index: number) {
	return info.label?.trim() || `Terminal ${index + 1}`;
}

function toSummary(
	info: TerminalInfoRecord,
	index: number,
	existing?: TerminalSummary,
): TerminalSummary {
	return {
		id: info.terminal,
		title: terminalTitle(info, index),
		status: existing?.status ?? "idle",
		cols: info.cols,
		rows: info.rows,
		createdAtUnixMs: info.created_at_unix_ms,
		attachedClients: info.attached_clients,
		error: existing?.error ?? null,
	};
}

function markReady(terminal: TerminalSummary): TerminalSummary {
	return {
		...terminal,
		status: "ready",
		error: null,
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
				const existing = new Map(
					state.terminals.map((terminal) => [terminal.id, terminal]),
				);
				const next = [...terminals]
					.sort(
						(left, right) =>
							left.created_at_unix_ms - right.created_at_unix_ms ||
							left.terminal.localeCompare(right.terminal),
					)
					.map((terminal, index) =>
						toSummary(terminal, index, existing.get(terminal.terminal)),
					);
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
		markTerminalConnecting: (terminalId) =>
			set((state) => ({
				activeSessionId: terminalId,
				terminals: state.terminals.map((terminal) =>
					terminal.id === terminalId
						? { ...terminal, status: "connecting", error: null }
						: terminal,
				),
			})),
		markTerminalReady: (info) =>
			set((state) => ({
				terminals: state.terminals.some(
					(terminal) => terminal.id === info.terminal,
				)
					? state.terminals.map((terminal, index) =>
							terminal.id === info.terminal
								? markReady(toSummary(info, index, terminal))
								: terminal,
						)
					: [
							...state.terminals,
							markReady(toSummary(info, state.terminals.length)),
						].sort(
							(left, right) =>
								left.createdAtUnixMs - right.createdAtUnixMs ||
								left.id.localeCompare(right.id),
						),
			})),
		markTerminalError: (terminalId, message) =>
			set((state) => ({
				terminals: state.terminals.map((terminal) =>
					terminal.id === terminalId
						? { ...terminal, status: "error", error: message }
						: terminal,
				),
			})),
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
