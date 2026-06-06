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

interface TerminalSessionRegistryDeps {
	controller: TerminalController;
	workspaceState: StoreApi<WorkspaceStoreShape>;
	session: () => SessionActor | null;
	onInlineError: (message: string | null) => void;
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

interface SessionState {
	phase: Phase;
	error: string | null;
	pendingOpen: Promise<void> | null;
	pendingHistory: Promise<TerminalHistoryPageRecord | null> | null;
	liveBuffer: BufferedLiveOutput[];
	loadedRange: { startSeq: number | null; endSeq: number | null };
	historyExhausted: boolean;
	terminalGeneration: number;
}

function createState(): SessionState {
	return {
		phase: "idle",
		error: null,
		pendingOpen: null,
		pendingHistory: null,
		liveBuffer: [],
		loadedRange: { startSeq: null, endSeq: null },
		historyExhausted: false,
		terminalGeneration: 0,
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

export class TerminalSessionRegistry {
	private selectedTerminalId: string | null = null;
	private connectionGeneration: number | null = null;
	private state = createState();

	constructor(private readonly deps: TerminalSessionRegistryDeps) {}

	applyOutput(
		terminalId: string,
		seq: number,
		chunk: string,
		connectionGeneration: number,
	) {
		if (
			terminalId !== this.selectedTerminalId ||
			connectionGeneration !== this.connectionGeneration
		) {
			return;
		}

		if (
			this.state.phase === "opening" ||
			this.state.phase === "loading_history"
		) {
			this.bufferLiveOutput({ seq, chunk, connectionGeneration });
			return;
		}

		this.deps.controller.appendActiveOutput(terminalId, chunk, seq);
	}

	connect(connectionGeneration: number) {
		this.connectionGeneration = connectionGeneration;
		this.activateSelectedTerminal();
	}

	disconnect(connectionGeneration: number) {
		this.connectionGeneration = connectionGeneration;
		this.state = createState();
		this.state.phase = "detached";
		this.state.terminalGeneration += 1;
		this.deps.controller.reset();
	}

	removeTerminal(terminalId: string) {
		if (this.selectedTerminalId !== terminalId) {
			return;
		}

		this.selectedTerminalId = null;
		this.state = createState();
		this.deps.controller.reset();
	}

	dispose() {}

	mountActive(host: HTMLElement) {
		if (this.selectedTerminalId == null) {
			return Promise.resolve();
		}
		return this.deps.controller.mount(host);
	}

	unmountActive() {
		this.deps.controller.unmount();
	}

	resizeActive(cols: number, rows: number) {
		if (this.selectedTerminalId == null) {
			return;
		}
		const session = this.deps.session();
		if (
			session == null ||
			(this.state.phase !== "opening" && this.state.phase !== "ready")
		) {
			return;
		}

		this.deps.workspaceState
			.getState()
			.updateTerminalSize(this.selectedTerminalId, cols, rows);
		void session
			.resize(this.selectedTerminalId, cols, rows)
			.catch((error: unknown) => {
				this.deps.onInlineError(
					error instanceof Error ? error.message : "failed to resize terminal",
				);
			});
	}

	loadOlderHistoryActive() {
		if (
			this.selectedTerminalId == null ||
			this.state.pendingOpen != null ||
			this.state.pendingHistory != null ||
			this.state.phase !== "ready" ||
			this.state.historyExhausted ||
			this.state.loadedRange.startSeq == null ||
			this.state.loadedRange.startSeq <= 0
		) {
			return;
		}

		const session = this.deps.session();
		if (session == null) {
			return;
		}

		const connectionGeneration = this.connectionGeneration;
		const terminalGeneration = this.state.terminalGeneration;
		const before = this.state.loadedRange.startSeq;
		this.state.phase = "loading_history";

		const history = this.loadHistoryPage(
			session,
			before,
			connectionGeneration,
			terminalGeneration,
		);
		this.state.pendingHistory = history;
		void history.finally(() => {
			if (
				this.connectionGeneration === connectionGeneration &&
				this.state.terminalGeneration === terminalGeneration
			) {
				this.state.pendingHistory = null;
			}
		});
	}

	retryActive() {
		this.activateSelectedTerminal();
	}

	activeRuntimeState() {
		if (this.selectedTerminalId == null) {
			return null;
		}
		return {
			phase: this.state.phase,
			error: this.state.phase === "failed" ? this.state.error : null,
		};
	}

	setSelectedTerminal(terminalId: string | null) {
		if (this.selectedTerminalId === terminalId) {
			return;
		}

		this.selectedTerminalId = terminalId;
		this.activateSelectedTerminal();
	}

	private activateSelectedTerminal() {
		if (this.selectedTerminalId == null || this.connectionGeneration == null) {
			return;
		}

		this.state = createState();
		this.state.phase = "opening";
		this.state.historyExhausted = false;
		this.state.terminalGeneration += 1;
		const terminalGeneration = this.state.terminalGeneration;
		const connectionGeneration = this.connectionGeneration;
		this.deps.controller.reset();
		this.deps.controller.setActiveTerminal(this.selectedTerminalId);

		const open = this.open(
			this.selectedTerminalId,
			connectionGeneration,
			terminalGeneration,
		);
		this.state.pendingOpen = open;
		void open.finally(() => {
			if (
				this.connectionGeneration === connectionGeneration &&
				this.state.terminalGeneration === terminalGeneration
			) {
				this.state.pendingOpen = null;
			}
		});
	}

	private async open(
		terminalId: string,
		connectionGeneration: number | null,
		terminalGeneration: number,
	) {
		const session = this.deps.session();
		if (session == null || connectionGeneration == null) {
			return;
		}

		try {
			const attachAck = await new Promise<unknown>((resolve, reject) => {
				const timeoutId = window.setTimeout(() => {
					reject(new Error("terminal open timed out"));
				}, terminalOpenTimeoutMs);
				void session
					.openTerminal(terminalId)
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

			this.state.loadedRange = {
				startSeq: parsed.start_seq,
				endSeq: parsed.end_seq,
			};
			this.state.historyExhausted = !parsed.has_more_history;
			this.deps.controller.renderSnapshot(
				terminalId,
				decodeBase64Bytes(parsed.render_bytes_b64),
				parsed.start_seq,
			);
			this.state.phase = "ready";
			this.replayBufferedOutput(connectionGeneration);
		} catch (error: unknown) {
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}
			const message =
				error instanceof Error ? error.message : "failed to open terminal";
			this.state.phase = "failed";
			this.state.error = message;
			if (message !== "terminal open timed out") {
				this.deps.onInlineError(message);
			}
		}
	}

	private isCurrent(connectionGeneration: number, terminalGeneration: number) {
		return (
			this.connectionGeneration === connectionGeneration &&
			this.state.terminalGeneration === terminalGeneration
		);
	}

	private async loadHistoryPage(
		session: SessionActor,
		before: number,
		connectionGeneration: number | null,
		terminalGeneration: number,
	) {
		if (connectionGeneration == null || this.selectedTerminalId == null) {
			return null;
		}

		try {
			const page = await session.readHistory(
				this.selectedTerminalId,
				before,
				initialHistoryPageBytes,
			);
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return null;
			}
			if (page.terminal_id !== this.selectedTerminalId) {
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
				this.state.phase = "ready";
				this.replayBufferedOutput(connectionGeneration);
			}
		}
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
		this.state.loadedRange.startSeq = page.start_seq;
		this.state.historyExhausted = !page.has_more;
	}

	private bufferLiveOutput(output: BufferedLiveOutput) {
		const lastBuffered =
			this.state.liveBuffer[this.state.liveBuffer.length - 1];
		if (lastBuffered == null || lastBuffered.seq <= output.seq) {
			this.state.liveBuffer.push(output);
			return;
		}

		let insertAt = this.state.liveBuffer.length - 1;
		while (insertAt >= 0 && this.state.liveBuffer[insertAt].seq > output.seq) {
			insertAt -= 1;
		}
		this.state.liveBuffer.splice(insertAt + 1, 0, output);
	}

	private replayBufferedOutput(connectionGeneration: number) {
		const replay = this.state.liveBuffer.filter(
			(output) => output.connectionGeneration === connectionGeneration,
		);
		this.state.liveBuffer = [];
		for (const output of replay) {
			this.deps.controller.appendActiveOutput(
				this.selectedTerminalId ?? "",
				output.chunk,
				output.seq,
			);
		}
	}
}
