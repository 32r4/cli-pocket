import type { StoreApi } from "zustand/vanilla";
import type {
	SessionActor,
	TerminalHistoryPageRecord,
	TerminalOpenAckRecord,
} from "@/platform/bridge/types";
import type { TerminalController } from "./terminalController";

const initialHistoryPageBytes = 32 * 1024;
const terminalOpenTimeoutMs = 10_000;

type Phase =
	| "idle"
	| "opening"
	| "ready"
	| "loading_history"
	| "failed"
	| "detached";

interface WorkspaceStoreShape {
	updateTerminalSize: (terminalId: string, cols: number, rows: number) => void;
}

interface TerminalSessionActorDeps {
	terminalId: string;
	controller: TerminalController;
	workspaceState: StoreApi<WorkspaceStoreShape>;
	session: () => SessionActor | null;
	onInlineError: (message: string | null) => void;
	onRuntimeStateChange: (
		terminalId: string,
		runtimeState: TerminalRuntimeState,
	) => void;
}

export interface TerminalRuntimeState {
	phase: Phase;
	error: string | null;
}

interface BufferedLiveOutput {
	seq: number;
	chunk: string;
	connectionGeneration: number;
}

function decodeBase64Bytes(value: string) {
	const binary = window.atob(value);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return new TextDecoder().decode(bytes);
}

function parseTerminalOpenAck(value: unknown): TerminalOpenAckRecord | null {
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
	const startSeq =
		"start_seq" in value && typeof value.start_seq === "number"
			? value.start_seq
			: null;
	const endSeq =
		"end_seq" in value && typeof value.end_seq === "number"
			? value.end_seq
			: null;
	const renderBytes =
		"render_bytes_b64" in value && typeof value.render_bytes_b64 === "string"
			? value.render_bytes_b64
			: null;
	const hasMoreHistory =
		"has_more_history" in value && typeof value.has_more_history === "boolean"
			? value.has_more_history
			: null;
	if (
		terminal == null ||
		startSeq == null ||
		endSeq == null ||
		endSeq < startSeq ||
		renderBytes == null ||
		hasMoreHistory == null
	) {
		return null;
	}

	return value as TerminalOpenAckRecord;
}

export class TerminalSessionActor {
	private connectionGeneration = 0;
	private terminalGeneration = 0;
	private phase: Phase = "idle";
	private pendingOpen: Promise<void> | null = null;
	private pendingHistory: Promise<TerminalHistoryPageRecord | null> | null =
		null;
	private liveBuffer: BufferedLiveOutput[] = [];
	private loadedRange: { startSeq: number | null; endSeq: number | null } = {
		startSeq: null,
		endSeq: null,
	};
	private historyExhausted = false;

	constructor(private readonly deps: TerminalSessionActorDeps) {}

	getRuntimeState(): TerminalRuntimeState {
		return {
			phase: this.phase,
			error: this.phase === "failed" ? this.lastError : null,
		};
	}

	private lastError: string | null = null;

	activateTerminal(connectionGeneration: number) {
		this.connectionGeneration = connectionGeneration;
		this.terminalGeneration += 1;
		const terminalGeneration = this.terminalGeneration;
		this.phase = "opening";
		this.lastError = null;
		this.historyExhausted = false;
		this.liveBuffer = [];
		this.deps.controller.setActiveTerminal(this.deps.terminalId);
		this.emitRuntimeState();

		const open = this.open(connectionGeneration, terminalGeneration);
		this.pendingOpen = open;
		void open.finally(() => {
			if (
				this.connectionGeneration === connectionGeneration &&
				this.terminalGeneration === terminalGeneration
			) {
				this.pendingOpen = null;
			}
		});
	}

	applyOutput(
		terminalId: string,
		seq: number,
		chunk: string,
		connectionGeneration: number,
	) {
		if (
			terminalId !== this.deps.terminalId ||
			connectionGeneration !== this.connectionGeneration
		) {
			return;
		}

		if (this.phase === "opening" || this.phase === "loading_history") {
			this.bufferLiveOutput({ seq, chunk, connectionGeneration });
			return;
		}

		this.deps.controller.appendActiveOutput(terminalId, chunk, seq);
	}

	loadOlderHistory() {
		if (
			this.pendingOpen != null ||
			this.pendingHistory != null ||
			this.phase !== "ready" ||
			this.historyExhausted ||
			this.loadedRange.startSeq == null ||
			this.loadedRange.startSeq <= 0
		) {
			return;
		}

		const session = this.deps.session();
		if (session == null) {
			return;
		}

		const connectionGeneration = this.connectionGeneration;
		const terminalGeneration = this.terminalGeneration;
		const before = this.loadedRange.startSeq;
		this.phase = "loading_history";
		const history = this.loadHistoryPage(
			session,
			before,
			connectionGeneration,
			terminalGeneration,
		);
		this.pendingHistory = history;
		void history.finally(() => {
			if (
				this.connectionGeneration === connectionGeneration &&
				this.terminalGeneration === terminalGeneration
			) {
				this.pendingHistory = null;
			}
		});
	}

	resize(cols: number, rows: number) {
		const session = this.deps.session();
		if (
			session == null ||
			(this.phase !== "opening" && this.phase !== "ready")
		) {
			return;
		}
		this.deps.workspaceState
			.getState()
			.updateTerminalSize(this.deps.terminalId, cols, rows);
		void session
			.resize(this.deps.terminalId, cols, rows)
			.catch((error: unknown) => {
				this.deps.onInlineError(
					error instanceof Error ? error.message : "failed to resize terminal",
				);
			});
	}

	detach() {
		this.phase = "detached";
		this.lastError = null;
		this.emitRuntimeState();
		this.terminalGeneration += 1;
	}

	disconnect(connectionGeneration: number) {
		this.connectionGeneration = connectionGeneration;
		this.terminalGeneration += 1;
		this.pendingOpen = null;
		this.pendingHistory = null;
		this.liveBuffer = [];
		this.loadedRange = { startSeq: null, endSeq: null };
		this.phase = "detached";
		this.lastError = null;
		this.emitRuntimeState();
	}

	mount(host: HTMLElement) {
		return this.deps.controller.mount(host);
	}

	unmount() {
		this.deps.controller.unmount();
	}

	private async open(connectionGeneration: number, terminalGeneration: number) {
		const session = this.deps.session();
		if (session == null) {
			return;
		}

		try {
			const attachAck = await new Promise<unknown>((resolve, reject) => {
				const timeoutId = window.setTimeout(() => {
					reject(new Error("terminal open timed out"));
				}, terminalOpenTimeoutMs);
				void session
					.openTerminal(this.deps.terminalId)
					.then(resolve, reject)
					.finally(() => {
						window.clearTimeout(timeoutId);
					});
			});
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}

			const parsed = parseTerminalOpenAck(attachAck);
			if (parsed == null) {
				throw new Error("invalid terminal open ack");
			}
			const initialWindow = {
				startSeq: parsed.start_seq,
				endSeq: parsed.end_seq,
				snapshot: decodeBase64Bytes(parsed.render_bytes_b64),
				historyExhausted: !parsed.has_more_history,
			};
			this.loadedRange = {
				startSeq: initialWindow.startSeq,
				endSeq: initialWindow.endSeq,
			};
			this.historyExhausted = initialWindow.historyExhausted;
			this.deps.controller.renderSnapshot(
				this.deps.terminalId,
				initialWindow.snapshot,
				initialWindow.startSeq,
			);
			this.phase = "ready";
			this.lastError = null;
			this.emitRuntimeState();
			this.replayBufferedOutput(connectionGeneration);
		} catch (error: unknown) {
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}
			const message =
				error instanceof Error ? error.message : "failed to open terminal";
			this.phase = "failed";
			this.lastError = message;
			this.emitRuntimeState();
			if (message !== "terminal open timed out") {
				this.deps.onInlineError(message);
			}
		}
	}

	private isCurrent(connectionGeneration: number, terminalGeneration: number) {
		return (
			this.connectionGeneration === connectionGeneration &&
			this.terminalGeneration === terminalGeneration
		);
	}

	private async loadHistoryPage(
		session: SessionActor,
		before: number,
		connectionGeneration: number,
		terminalGeneration: number,
	) {
		try {
			const page = await session.readHistory(
				this.deps.terminalId,
				before,
				initialHistoryPageBytes,
			);
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return null;
			}
			if (page.terminal_id !== this.deps.terminalId) {
				return null;
			}

			await this.applyHistoryPage(
				page,
				connectionGeneration,
				terminalGeneration,
			);
			return page;
		} catch (error: unknown) {
			if (this.isCurrent(connectionGeneration, terminalGeneration)) {
				this.deps.onInlineError(
					error instanceof Error ? error.message : "failed to load history",
				);
			}
			return null;
		} finally {
			if (this.isCurrent(connectionGeneration, terminalGeneration)) {
				this.phase = "ready";
				this.emitRuntimeState();
				this.replayBufferedOutput(connectionGeneration);
			}
		}
	}

	private emitRuntimeState() {
		this.deps.onRuntimeStateChange(
			this.deps.terminalId,
			this.getRuntimeState(),
		);
	}

	private async applyHistoryPage(
		page: TerminalHistoryPageRecord,
		connectionGeneration: number,
		terminalGeneration: number,
	) {
		if (page.bytes_b64.length > 0) {
			await this.deps.controller.prependHistoryPage(page);
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}
		}
		this.loadedRange.startSeq = page.start_seq;
		this.historyExhausted = !page.has_more;
	}

	private bufferLiveOutput(output: BufferedLiveOutput) {
		const lastBuffered = this.liveBuffer[this.liveBuffer.length - 1];
		if (lastBuffered == null || lastBuffered.seq <= output.seq) {
			this.liveBuffer.push(output);
			return;
		}

		let insertAt = this.liveBuffer.length - 1;
		while (insertAt >= 0 && this.liveBuffer[insertAt].seq > output.seq) {
			insertAt -= 1;
		}
		this.liveBuffer.splice(insertAt + 1, 0, output);
	}

	private replayBufferedOutput(connectionGeneration: number) {
		const replay = this.liveBuffer.filter(
			(output) => output.connectionGeneration === connectionGeneration,
		);
		this.liveBuffer = [];
		for (const output of replay) {
			this.deps.controller.appendActiveOutput(
				this.deps.terminalId,
				output.chunk,
				output.seq,
			);
		}
	}
}
