import { LoaderCircle, Plus, X } from "lucide-react";
import { useEffect, useRef } from "react";
import type { StoreApi } from "zustand/vanilla";
import { TerminalViewport } from "@/features/terminals/TerminalViewport";
import type { TerminalController } from "@/features/terminals/terminalController";
import type {
	SessionActor,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
import type { ConnectionState } from "@/state/workspace/workspaceState";

interface TerminalSummaryView {
	id: string;
	title: string;
	status: "idle" | "connecting" | "ready" | "error";
	cols: number;
	rows: number;
	error: string | null;
}

interface TerminalAreaProps {
	session: SessionActor | null;
	workspace: {
		connectionState: ConnectionState;
		terminals: TerminalSummaryView[];
		activeSessionId: string | null;
	};
	workspaceState: StoreApi<{
		terminals: TerminalSummaryView[];
		setActiveSessionId: (terminalId: string | null) => void;
		markTerminalConnecting: (terminalId: string) => void;
		markTerminalReady: (info: TerminalSnapshotRecord["info"]) => void;
		markTerminalError: (terminalId: string, message: string) => void;
		updateTerminalSize: (
			terminalId: string,
			cols: number,
			rows: number,
		) => void;
		removeTerminal: (terminalId: string) => void;
	}>;
	controller: TerminalController;
	theme: "light" | "dark";
	onInlineError: (message: string | null) => void;
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
			? value.info
			: null;
	if (info == null) {
		return null;
	}

	const terminal =
		"terminal" in info && typeof info.terminal === "string"
			? info.terminal
			: null;
	const snapshotBytes =
		"snapshot_bytes_b64" in value &&
		typeof value.snapshot_bytes_b64 === "string"
			? value.snapshot_bytes_b64
			: null;
	if (terminal == null || snapshotBytes == null) {
		return null;
	}

	return value as TerminalSnapshotRecord;
}

export async function openTerminalSnapshot({
	session,
	terminalId,
	onMarkTerminalConnecting,
	onMarkTerminalReady,
	onMarkTerminalError,
	onInlineError,
	onRenderSnapshot,
}: {
	session: SessionActor | null;
	terminalId: string;
	onMarkTerminalConnecting: (terminalId: string) => void;
	onMarkTerminalReady: (snapshot: TerminalSnapshotRecord) => void;
	onMarkTerminalError: (terminalId: string, message: string) => void;
	onInlineError: (message: string | null) => void;
	onRenderSnapshot: (terminalId: string, snapshot: string) => void;
}) {
	onMarkTerminalConnecting(terminalId);
	if (session == null) {
		return;
	}

	try {
		const snapshot = await Promise.race([
			session.openTerminal(terminalId),
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
		onMarkTerminalReady(parsed);
		onRenderSnapshot(terminalId, decodeBase64Bytes(parsed.snapshot_bytes_b64));
	} catch (error: unknown) {
		const message =
			error instanceof Error ? error.message : "failed to open terminal";
		onMarkTerminalError(terminalId, message);
		onInlineError(message);
	}
}

export function TerminalArea({
	session,
	workspace,
	workspaceState,
	controller,
	theme,
	onInlineError,
}: TerminalAreaProps) {
	const lastOpenedTerminalIdRef = useRef<string | null>(null);
	const activeSession =
		workspace.terminals.find(
			(terminal) => terminal.id === workspace.activeSessionId,
		) ?? null;

	useEffect(() => {
		controller.setTheme(theme);
	}, [controller, theme]);

	useEffect(() => {
		controller.setActiveTerminal(activeSession?.id ?? null);
	}, [activeSession?.id, controller]);

	useEffect(() => {
		if (activeSession == null) {
			lastOpenedTerminalIdRef.current = null;
			return;
		}
		if (activeSession.status === "connecting") {
			return;
		}
		if (lastOpenedTerminalIdRef.current === activeSession.id) {
			return;
		}

		lastOpenedTerminalIdRef.current = activeSession.id;
		void openTerminalSnapshot({
			session,
			terminalId: activeSession.id,
			onMarkTerminalConnecting: (terminalId) => {
				workspaceState.getState().markTerminalConnecting(terminalId);
			},
			onMarkTerminalReady: (snapshot) => {
				workspaceState.getState().markTerminalReady(snapshot.info);
			},
			onMarkTerminalError: (terminalId, message) => {
				workspaceState.getState().markTerminalError(terminalId, message);
				lastOpenedTerminalIdRef.current = null;
			},
			onInlineError,
			onRenderSnapshot: (terminalId, snapshot) => {
				controller.renderSnapshot(terminalId, snapshot);
			},
		});
	}, [activeSession, controller, onInlineError, session, workspaceState]);

	useEffect(() => {
		controller.setHandlers({
			onInput: (terminalId, data) => {
				if (session == null) {
					return;
				}

				void session
					.sendInput(terminalId, new TextEncoder().encode(data))
					.catch((error: unknown) => {
						onInlineError(
							error instanceof Error ? error.message : "failed to send input",
						);
					});
			},
			onResize: (terminalId, cols, rows) => {
				if (session == null) {
					return;
				}

				workspaceState.getState().updateTerminalSize(terminalId, cols, rows);
				void session.resize(terminalId, cols, rows).catch((error: unknown) => {
					onInlineError(
						error instanceof Error
							? error.message
							: "failed to resize terminal",
					);
				});
			},
		});
	}, [controller, onInlineError, session, workspaceState]);

	if (workspace.connectionState !== "connected") {
		return null;
	}

	const createSession = async () => {
		if (session == null || workspace.connectionState !== "connected") {
			return;
		}

		try {
			const createdTerminal = await session.createTerminal({
				cols: 120,
				rows: 36,
			});
			if (createdTerminal != null) {
				workspaceState.getState().setActiveSessionId(createdTerminal.terminal);
			}
		} catch (error: unknown) {
			onInlineError(
				error instanceof Error ? error.message : "failed to create terminal",
			);
		}
	};

	const selectTerminal = async (terminalId: string) => {
		workspaceState.getState().setActiveSessionId(terminalId);
	};

	const killTerminal = async (terminalId: string) => {
		if (session == null) {
			return;
		}

		workspaceState.getState().removeTerminal(terminalId);
		try {
			await session.kill(terminalId, "TERM");
		} catch (error: unknown) {
			await session.refreshTerminals().catch(() => undefined);
			onInlineError(
				error instanceof Error ? error.message : "failed to kill terminal",
			);
		}
	};

	return (
		<section className="workspace-panel" aria-label="Terminal workspace">
			<div className="terminal-tabs" role="tablist" aria-label="Sessions">
				<div className="terminal-tabs__list">
					{workspace.terminals.map((terminal) => (
						<div
							className="terminal-tab"
							key={terminal.id}
							data-active={terminal.id === activeSession?.id}
						>
							<button
								className="terminal-tab__button"
								type="button"
								role="tab"
								aria-selected={terminal.id === activeSession?.id}
								onClick={() => {
									void selectTerminal(terminal.id);
								}}
							>
								<span className="terminal-tab__label">{terminal.title}</span>
							</button>
							<button
								className="icon-button terminal-tab__close"
								type="button"
								aria-label={`Kill ${terminal.title}`}
								onClick={(event) => {
									event.stopPropagation();
									void killTerminal(terminal.id);
								}}
							>
								<X aria-hidden="true" size={14} strokeWidth={1.75} />
							</button>
						</div>
					))}
				</div>
				<div className="terminal-tabs__actions">
					<button
						className="icon-button terminal-tabs__add"
						type="button"
						aria-label="Create terminal"
						onClick={() => {
							void createSession();
						}}
					>
						<Plus aria-hidden="true" size={16} strokeWidth={1.75} />
					</button>
				</div>
			</div>
			<div className="terminal-stage">
				{activeSession == null ? (
					<div className="xterm-server">Select a terminal</div>
				) : activeSession.status === "connecting" ? (
					<div className="xterm-server xterm-server--spinner">
						<LoaderCircle
							className="connection-spinner"
							aria-hidden="true"
							size={32}
							strokeWidth={1.75}
						/>
						<span className="sr-only">Connecting terminal</span>
					</div>
				) : activeSession.status === "error" ? (
					<div className="xterm-server">
						{activeSession.error ?? "Terminal attach failed"}
					</div>
				) : (
					<TerminalViewport controller={controller} />
				)}
			</div>
			<footer className="terminal-footer">
				<span className="terminal-footer__title">
					{activeSession?.title ?? "No terminal"}
				</span>
				<span className="terminal-footer__size">
					{activeSession == null
						? "--"
						: `${activeSession.cols}x${activeSession.rows}`}
				</span>
				<span className="terminal-footer__state">
					{activeSession?.status ?? workspace.connectionState}
				</span>
			</footer>
		</section>
	);
}
