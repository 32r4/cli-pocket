import { LoaderCircle, Plus, X } from "lucide-react";
import { useEffect } from "react";
import type { StoreApi } from "zustand/vanilla";
import { TerminalViewport } from "@/features/terminals/TerminalViewport";
import type { TerminalController } from "@/features/terminals/terminalController";
import type { TerminalSessionRegistry } from "@/features/terminals/terminalSessionRegistry";
import type { SessionActor } from "@/platform/bridge/types";
import type { ConnectionState } from "@/state/workspace/workspaceState";
import { TerminalControlBar } from "./TerminalControlBar";

interface TerminalSummaryView {
	id: string;
	title: string;
	cols: number;
	rows: number;
}

interface TerminalAreaProps {
	session: SessionActor | null;
	workspace: {
		connectionState: ConnectionState;
		terminals: TerminalSummaryView[];
		activeSessionId: string | null;
	};
	isCompactViewport: boolean;
	showTerminalControlBar: boolean;
	terminalFontSize: number;
	workspaceState: StoreApi<{
		terminals: TerminalSummaryView[];
		setActiveSessionId: (terminalId: string | null) => void;
		removeTerminal: (terminalId: string) => void;
	}>;
	controller: TerminalController;
	registry: TerminalSessionRegistry;
	theme: "light" | "dark";
	onInlineError: (message: string | null) => void;
}

export function TerminalArea({
	session,
	workspace,
	isCompactViewport,
	showTerminalControlBar,
	terminalFontSize,
	workspaceState,
	controller,
	registry,
	theme,
	onInlineError,
}: TerminalAreaProps) {
	const defaultTerminalSize = { cols: 120, rows: 36 };
	const activeSession =
		workspace.terminals.find(
			(terminal) => terminal.id === workspace.activeSessionId,
		) ?? null;
	const activeRuntimeState = registry.activeRuntimeState();
	const isConnecting = activeRuntimeState?.phase === "opening";
	const isError = activeRuntimeState?.phase === "failed";

	useEffect(() => {
		controller.setTheme(theme);
	}, [controller, theme]);

	useEffect(() => {
		controller.setCompactMode(isCompactViewport);
	}, [controller, isCompactViewport]);

	useEffect(() => {
		controller.setTerminalFontSize(terminalFontSize);
	}, [controller, terminalFontSize]);

	useEffect(() => {
		registry.setSelectedTerminal(workspace.activeSessionId);
	}, [registry, workspace.activeSessionId]);

	if (workspace.connectionState !== "connected") {
		return null;
	}

	const createSession = async () => {
		if (session == null || workspace.connectionState !== "connected") {
			return;
		}

		try {
			const terminalSize =
				(await registry.measureViewportSize()) ?? defaultTerminalSize;
			const createdTerminal = await session.createTerminal(terminalSize);
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
		registry.removeTerminal(terminalId);
		controller.removeTerminal(terminalId);

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
		<section
			className="workspace-panel"
			data-terminal-controls={showTerminalControlBar ? "true" : undefined}
			aria-label="Terminal workspace"
		>
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
						<Plus aria-hidden="true" size={14} strokeWidth={1.75} />
					</button>
				</div>
			</div>
			<div className="terminal-stage">
				<TerminalViewport registry={registry} />
				{activeSession == null ? null : isConnecting ? (
					<div className="terminal-stage__overlay" aria-live="polite">
						<div className="xterm-server xterm-server--spinner">
							<LoaderCircle
								className="connection-spinner"
								aria-hidden="true"
								size={32}
								strokeWidth={1.75}
							/>
							<span className="sr-only">Connecting terminal</span>
						</div>
					</div>
				) : isError ? (
					<div className="terminal-stage__overlay" aria-live="polite">
						<div className="xterm-server xterm-server--spinner">
							<button
								className="retry-button"
								type="button"
								aria-label="Retry"
								onClick={() => {
									registry.retryActive();
								}}
							>
								Retry
							</button>
						</div>
					</div>
				) : null}
			</div>
			{showTerminalControlBar ? (
				<TerminalControlBar
					controller={controller}
					onInlineError={onInlineError}
				/>
			) : null}
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
					{activeRuntimeState?.phase ?? workspace.connectionState}
				</span>
			</footer>
		</section>
	);
}
