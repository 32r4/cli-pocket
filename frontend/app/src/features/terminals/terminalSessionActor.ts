import type { StoreApi } from "zustand/vanilla";
import type {
	SessionActor,
	TerminalHistoryPageRecord,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
import type { TerminalController } from "./terminalController";

const initialHistoryPageBytes = 32 * 1024;
const initialHistoryTargetBytes = 128 * 1024;
const maxInitialHistoryPages = 4;

type Phase =
	| "idle"
	| "opening"
	| "ready"
	| "loading_history"
	| "failed"
	| "detached";

interface WorkspaceStoreShape {
	markTerminalConnecting: (terminalId: string) => void;
	markTerminalReady: (info: TerminalSnapshotRecord["info"]) => void;
	markTerminalError: (terminalId: string, message: string) => void;
	updateTerminalSize: (terminalId: string, cols: number, rows: number) => void;
}

interface TerminalSessionActorDeps {
	terminalId: string;
	controller: TerminalController;
	workspaceState: StoreApi<WorkspaceStoreShape>;
	session: () => SessionActor | null;
	onInlineError: (message: string | null) => void;
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

async function preloadTerminalWindow(
	session: SessionActor,
	snapshot: TerminalSnapshotRecord,
) {
	const renderPrefix = decodeBase64Bytes(snapshot.render_prefix_b64);
	let startSeq = snapshot.start_seq;
	const historyChunks: string[] = [];
	const snapshotBytes = decodeBase64Bytes(snapshot.snapshot_bytes_b64);
	let loadedBytes = snapshotBytes.length;
	let nextBefore = snapshot.start_seq;

	for (
		let page = 0;
		page < maxInitialHistoryPages &&
		nextBefore > 0 &&
		loadedBytes < initialHistoryTargetBytes;
		page += 1
	) {
		const history = await session.readHistory(
			snapshot.info.terminal,
			nextBefore,
			initialHistoryPageBytes,
		);
		if (history.bytes_b64.length === 0) {
			startSeq = history.start_seq;
			break;
		}

		const chunk = decodeBase64Bytes(history.bytes_b64);
		if (chunk.length === 0 || history.start_seq >= nextBefore) {
			break;
		}

		historyChunks.unshift(chunk);
		loadedBytes += chunk.length;
		startSeq = history.start_seq;
		nextBefore = history.start_seq;
		if (history.start_seq === 0) {
			break;
		}
	}

	return {
		startSeq,
		snapshot: `${renderPrefix}${historyChunks.join("")}${snapshotBytes}`,
	};
}

export class TerminalSessionActor {
	private connectionGeneration = 0;
	private terminalGeneration = 0;
	private phase: Phase = "idle";
	private pendingOpen: Promise<void> | null = null;
	private pendingHistory: Promise<void> | null = null;
	private liveBuffer: BufferedLiveOutput[] = [];
	private loadedRange: { startSeq: number | null; endSeq: number | null } = {
		startSeq: null,
		endSeq: null,
	};
	private historyExhausted = false;

	constructor(private readonly deps: TerminalSessionActorDeps) {}

	activateTerminal(connectionGeneration: number) {
		this.connectionGeneration = connectionGeneration;
		this.terminalGeneration += 1;
		const terminalGeneration = this.terminalGeneration;
		this.phase = "opening";
		this.historyExhausted = false;
		this.liveBuffer = [];
		this.deps.controller.setActiveTerminal(this.deps.terminalId);
		this.deps.workspaceState
			.getState()
			.markTerminalConnecting(this.deps.terminalId);

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
			const snapshot = await Promise.race([
				session.activateTerminal(this.deps.terminalId),
				new Promise<never>((_, reject) => {
					window.setTimeout(
						() => reject(new Error("terminal open timed out")),
						5_000,
					);
				}),
			]);
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}

			const parsed = parseTerminalSnapshot(snapshot);
			if (parsed == null) {
				throw new Error("invalid terminal snapshot");
			}
			let initialWindow = {
				startSeq: parsed.start_seq,
				snapshot: `${decodeBase64Bytes(parsed.render_prefix_b64)}${decodeBase64Bytes(parsed.snapshot_bytes_b64)}`,
			};
			try {
				initialWindow = await preloadTerminalWindow(session, parsed);
			} catch {
				// Fall back to the attach snapshot if preloading shared history fails.
			}
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}

			this.deps.workspaceState.getState().markTerminalReady(parsed.info);
			this.loadedRange = {
				startSeq: initialWindow.startSeq,
				endSeq: parsed.end_seq,
			};
			this.deps.controller.renderSnapshotWithRange(
				this.deps.terminalId,
				initialWindow.snapshot,
				initialWindow.startSeq,
			);
			this.phase = "ready";
			this.replayBufferedOutput(connectionGeneration);
		} catch (error: unknown) {
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}
			const message =
				error instanceof Error ? error.message : "failed to open terminal";
			this.phase = "failed";
			this.deps.workspaceState
				.getState()
				.markTerminalError(this.deps.terminalId, message);
			this.deps.onInlineError(message);
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
				return;
			}
			if (page.terminal_id !== this.deps.terminalId) {
				return;
			}

			await this.applyHistoryPage(
				page,
				connectionGeneration,
				terminalGeneration,
			);
		} catch (error: unknown) {
			if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
				return;
			}
			this.deps.onInlineError(
				error instanceof Error ? error.message : "failed to load history",
			);
		} finally {
			if (this.isCurrent(connectionGeneration, terminalGeneration)) {
				this.phase = "ready";
				this.replayBufferedOutput(connectionGeneration);
			}
		}
	}

	private async applyHistoryPage(
		page: TerminalHistoryPageRecord,
		connectionGeneration: number,
		terminalGeneration: number,
	) {
		if (page.bytes_b64.length === 0) {
			this.historyExhausted = true;
			this.loadedRange.startSeq = page.start_seq;
			return;
		}

		await this.deps.controller.prependHistoryPage(page);
		if (!this.isCurrent(connectionGeneration, terminalGeneration)) {
			return;
		}

		this.loadedRange.startSeq = page.start_seq;
		this.historyExhausted = page.start_seq === 0;
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
