import type { TerminalHistoryPageRecord } from "@/platform/bridge/types";
import type { ThemeName } from "@/state/ui/uiState";
import { TerminalReplica } from "./terminalReplica";

interface TerminalAddonLike {
	activate: (terminal: unknown) => void;
	dispose: () => void;
}

interface TerminalScrollDisposable {
	dispose: () => void;
}

interface TerminalBufferLike {
	readonly viewportY: number;
	readonly baseY: number;
	readonly length: number;
	getLine: (y: number) =>
		| {
				isWrapped: boolean;
				translateToString: (trimRight?: boolean) => string;
		  }
		| undefined;
}

interface TerminalLike {
	cols: number;
	rows: number;
	open: (node: HTMLElement) => void;
	write: (data: string, callback?: () => void) => void;
	clear: () => void;
	reset: () => void;
	focus: () => void;
	loadAddon: (addon: TerminalAddonLike) => void;
	scrollToLine: (line: number) => void;
	onScroll: (listener: (position: number) => void) => TerminalScrollDisposable;
	buffer: {
		active: TerminalBufferLike;
	};
	optionsService?: {
		options: {
			theme?: Record<string, string>;
		};
	};
	onData: (listener: (data: string) => void) => { dispose: () => void };
	dispose: () => void;
}

interface FitAddonLike extends TerminalAddonLike {
	fit: () => void;
}

interface TerminalModules {
	Terminal: new (options?: unknown) => TerminalLike;
	FitAddon: new () => FitAddonLike;
}

let terminalModulesPromise: Promise<
	[
		{ Terminal: TerminalModules["Terminal"] },
		{ FitAddon: TerminalModules["FitAddon"] },
	]
> | null = null;
const utf8Decoder = new TextDecoder();
const terminalScrollbackLines = 100_000;

interface TerminalControllerOptions {
	onInput: (terminalId: string, data: string) => void;
	onResize: (terminalId: string, cols: number, rows: number) => void;
	onLoadOlderHistory: () => void;
}

function decodeBase64Bytes(value: string) {
	const binary = window.atob(value);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return utf8Decoder.decode(bytes);
}

function readThemeToken(name: string) {
	if (typeof window === "undefined") {
		return "";
	}

	return getComputedStyle(document.documentElement)
		.getPropertyValue(name)
		.trim();
}

function terminalTheme() {
	return {
		background: readThemeToken("--surface-terminal"),
		foreground: readThemeToken("--terminal-fg"),
		cursor: readThemeToken("--terminal-cursor"),
		selectionBackground: readThemeToken("--terminal-selection-bg"),
	};
}

function writeToTerminal(terminal: TerminalLike, data: string) {
	return new Promise<void>((resolve) => {
		if (data.length === 0) {
			resolve();
			return;
		}
		terminal.write(data, () => {
			resolve();
		});
	});
}

function countBufferWrappedRows(buffer: TerminalBufferLike) {
	let wrappedRows = 0;
	for (let line = 0; line < buffer.length; line += 1) {
		if (buffer.getLine(line)?.isWrapped) {
			wrappedRows += 1;
		}
	}
	return wrappedRows;
}

function visibleLineCount(buffer: TerminalBufferLike) {
	return buffer.length - countBufferWrappedRows(buffer);
}

function countWrappedRowsInText(text: string, cols: number) {
	const lines = text.split("\n");
	let wrappedRows = 0;
	for (const line of lines) {
		const displayWidth = Math.max(1, line.length);
		wrappedRows += Math.max(0, Math.ceil(displayWidth / Math.max(1, cols)) - 1);
	}
	return wrappedRows;
}

function estimateVisibleLineCount(text: string, cols: number) {
	if (text.length === 0) {
		return 0;
	}
	const logicalLines = text.split("\n").length;
	return logicalLines + countWrappedRowsInText(text, cols);
}

function hasAnsiOrControlSequences(text: string) {
	for (let index = 0; index < text.length; index += 1) {
		const code = text.charCodeAt(index);
		if (
			code === 0x1b ||
			code === 0x9b ||
			code === 0x0d ||
			code === 0x08 ||
			code === 0x0c ||
			code === 0x0b
		) {
			return true;
		}
	}
	return false;
}

async function loadTerminalModules() {
	if (terminalModulesPromise == null) {
		terminalModulesPromise = Promise.all([
			import("@xterm/xterm"),
			import("@xterm/addon-fit"),
		]) as Promise<
			[
				{ Terminal: TerminalModules["Terminal"] },
				{ FitAddon: TerminalModules["FitAddon"] },
			]
		>;
	}

	return terminalModulesPromise;
}

export class TerminalController {
	private readonly replica = new TerminalReplica();
	private onInput: TerminalControllerOptions["onInput"];
	private onResize: TerminalControllerOptions["onResize"];
	private onLoadOlderHistory: TerminalControllerOptions["onLoadOlderHistory"];
	private activeTerminalId: string | null = null;
	private host: HTMLElement | null = null;
	private terminal: TerminalLike | null = null;
	private fitAddon: FitAddonLike | null = null;
	private dataSubscription: { dispose: () => void } | null = null;
	private scrollSubscription: TerminalScrollDisposable | null = null;
	private removePointerFocusListener: (() => void) | null = null;
	private removeWindowResizeListener: (() => void) | null = null;
	private lastReportedSize: { cols: number; rows: number } | null = null;
	private initPromise: Promise<void> | null = null;
	private probeHost: HTMLDivElement | null = null;
	private probeTerminal: TerminalLike | null = null;
	private probeFitAddon: FitAddonLike | null = null;

	constructor({
		onInput,
		onResize,
		onLoadOlderHistory,
	}: TerminalControllerOptions) {
		this.onInput = onInput;
		this.onResize = onResize;
		this.onLoadOlderHistory = onLoadOlderHistory;
	}

	reset() {
		this.activeTerminalId = null;
		this.replica.reset();
		this.terminal?.reset();
	}

	setTheme(_theme: ThemeName) {
		if (this.terminal?.optionsService?.options != null) {
			this.terminal.optionsService.options.theme = terminalTheme();
		}
	}

	setActiveTerminal(terminalId: string | null) {
		this.activeTerminalId = terminalId;
		this.flushPendingState();
		this.focusActiveTerminal();
	}

	currentRenderedText() {
		return this.replica.currentRenderedText();
	}

	getLoadedRange() {
		return this.replica.getLoadedRange();
	}

	renderSnapshot(
		terminalId: string,
		snapshot: string,
		startSeq: number | null,
	) {
		this.replica.queueSnapshot(terminalId, snapshot, startSeq);
		this.flushPendingState();
	}

	appendActiveOutput(terminalId: string, chunk: string, seq: number) {
		if (this.activeTerminalId !== terminalId || chunk.length === 0) {
			return;
		}

		if (this.terminal === null) {
			this.replica.queueDetachedLiveOutput(seq, chunk);
			return;
		}

		if (this.replica.isCurrentlyRedrawing()) {
			this.replica.bufferLiveOutput(seq, chunk);
			return;
		}

		this.appendLiveOutput(chunk, seq);
	}

	async prependHistoryPage(page: TerminalHistoryPageRecord) {
		await this.prependHistoryPageToTerminal(page);
	}

	removeTerminal(terminalId: string) {
		if (this.activeTerminalId === terminalId) {
			this.reset();
		}
	}

	async mount(host: HTMLElement) {
		this.host = host;
		await this.ensureTerminal();
	}

	unmount() {
		this.detachTerminal();
		this.host = null;
	}

	private async ensureTerminal() {
		if (this.host === null || this.terminal !== null) {
			return;
		}
		if (this.initPromise != null) {
			await this.initPromise;
			return;
		}

		this.initPromise = this.createTerminal();
		try {
			await this.initPromise;
		} finally {
			this.initPromise = null;
		}
	}

	private focusActiveTerminal() {
		if (this.terminal == null || this.activeTerminalId == null) {
			return;
		}

		window.requestAnimationFrame(() => {
			if (this.activeTerminalId != null) {
				this.terminal?.focus();
			}
		});
	}

	private async createTerminal() {
		if (this.host === null) {
			return;
		}

		const modules = await loadTerminalModules();
		const [{ Terminal }, { FitAddon }] = modules;

		if (this.host === null) {
			return;
		}

		const terminal = new Terminal({
			cols: 120,
			cursorBlink: true,
			rows: 32,
			scrollback: terminalScrollbackLines,
			theme: terminalTheme(),
		});
		const fitAddon = new FitAddon();

		terminal.loadAddon(fitAddon);
		terminal.open(this.host);
		this.terminal = terminal;
		this.fitAddon = fitAddon;
		this.dataSubscription = terminal.onData((data: string) => {
			if (this.activeTerminalId != null) {
				this.onInput(this.activeTerminalId, data);
			}
		});
		this.scrollSubscription = terminal.onScroll((viewportY) => {
			if (viewportY <= 0) {
				this.onLoadOlderHistory();
			}
		});

		const focusTerminal = () => {
			this.terminal?.focus();
		};
		const handleWindowResize = () => {
			window.requestAnimationFrame(() => {
				this.fitToViewport();
			});
		};

		this.host.addEventListener("pointerdown", focusTerminal);
		window.addEventListener("resize", handleWindowResize);
		this.removePointerFocusListener = () => {
			this.host?.removeEventListener("pointerdown", focusTerminal);
		};
		this.removeWindowResizeListener = () => {
			window.removeEventListener("resize", handleWindowResize);
		};

		window.requestAnimationFrame(() => {
			this.fitToViewport();
			this.terminal?.focus();
		});
		this.flushPendingState();
	}

	private detachTerminal() {
		if (
			this.activeTerminalId != null &&
			!this.replica.hasPendingSnapshot(this.activeTerminalId)
		) {
			const current = this.replica.currentRenderedText();
			const range = this.replica.getLoadedRange();
			if (current.length > 0 || range.startSeq != null) {
				this.replica.queueSnapshot(
					this.activeTerminalId,
					current,
					range.startSeq,
				);
			}
		}

		this.scrollSubscription?.dispose();
		this.scrollSubscription = null;
		this.removePointerFocusListener?.();
		this.removePointerFocusListener = null;
		this.removeWindowResizeListener?.();
		this.removeWindowResizeListener = null;
		this.dataSubscription?.dispose();
		this.dataSubscription = null;
		this.terminal?.dispose();
		this.probeTerminal?.dispose();
		this.probeTerminal = null;
		this.probeFitAddon = null;
		this.probeHost?.remove();
		this.probeHost = null;
		this.terminal = null;
		this.fitAddon = null;
		this.lastReportedSize = null;
	}

	private flushPendingState() {
		if (this.terminal == null || this.activeTerminalId == null) {
			return;
		}

		const pendingSnapshot = this.replica.consumePendingSnapshot(
			this.activeTerminalId,
		);
		if (pendingSnapshot != null) {
			this.applySnapshot(
				pendingSnapshot.terminalId,
				pendingSnapshot.snapshot,
				pendingSnapshot.startSeq,
			);
			return;
		}

		const replay = this.replica.drainDetachedLiveOutput();
		if (replay.length === 0) {
			return;
		}

		for (const output of replay) {
			this.appendLiveOutput(output.chunk, output.seq);
		}
	}

	private applySnapshot(
		terminalId: string,
		snapshot: string,
		startSeq: number | null,
	) {
		if (this.terminal === null || this.activeTerminalId !== terminalId) {
			return;
		}

		this.replica.setSnapshotContent(snapshot, startSeq);
		this.terminal.reset();
		if (snapshot.length > 0) {
			this.terminal.write(snapshot);
		}
		const replay = this.replica.drainDetachedLiveOutput();
		for (const output of replay) {
			this.appendLiveOutput(output.chunk, output.seq);
		}
		this.fitToViewport();
	}

	private fitToViewport() {
		if (
			this.terminal === null ||
			this.fitAddon === null ||
			this.activeTerminalId == null
		) {
			return;
		}

		this.fitAddon.fit();
		if (
			this.lastReportedSize?.cols === this.terminal.cols &&
			this.lastReportedSize?.rows === this.terminal.rows
		) {
			return;
		}

		this.lastReportedSize = {
			cols: this.terminal.cols,
			rows: this.terminal.rows,
		};
		this.onResize(
			this.activeTerminalId,
			this.terminal.cols,
			this.terminal.rows,
		);
	}

	private appendLiveOutput(chunk: string, seq: number) {
		if (this.terminal === null) {
			return;
		}

		this.replica.appendLiveChunk(chunk, seq);
		this.terminal.write(chunk);
	}

	private async ensureProbeTerminal() {
		if (this.host == null || this.terminal == null) {
			return null;
		}

		if (
			this.probeHost != null &&
			this.probeTerminal != null &&
			this.probeFitAddon != null
		) {
			this.probeHost.style.width = `${this.host.clientWidth || 1}px`;
			this.probeHost.style.height = `${this.host.clientHeight || 1}px`;
			this.probeFitAddon.fit();
			return this.probeTerminal;
		}

		const modules = await loadTerminalModules();
		const [{ Terminal }, { FitAddon }] = modules;
		const probeHost = document.createElement("div");
		probeHost.style.position = "absolute";
		probeHost.style.left = "-99999px";
		probeHost.style.top = "0";
		probeHost.style.width = `${this.host.clientWidth || 1}px`;
		probeHost.style.height = `${this.host.clientHeight || 1}px`;
		probeHost.style.visibility = "hidden";
		document.body.append(probeHost);

		const probeTerminal = new Terminal({
			cols: this.terminal.cols,
			rows: this.terminal.rows,
			scrollback: terminalScrollbackLines,
			theme: terminalTheme(),
		});
		const probeFitAddon = new FitAddon();
		probeTerminal.loadAddon(probeFitAddon);
		probeTerminal.open(probeHost);
		probeFitAddon.fit();

		this.probeHost = probeHost;
		this.probeTerminal = probeTerminal;
		this.probeFitAddon = probeFitAddon;
		return probeTerminal;
	}

	private async prependHistoryPageToTerminal(page: TerminalHistoryPageRecord) {
		if (
			this.terminal == null ||
			this.activeTerminalId == null ||
			page.terminal_id !== this.activeTerminalId
		) {
			return;
		}

		const chunk = decodeBase64Bytes(page.bytes_b64);
		if (chunk.length === 0) {
			return;
		}

		const previousViewportY = this.terminal.buffer.active.viewportY;
		this.replica.beginRedraw();
		const previousLiveBuffered = this.replica.drainBufferedLiveOutput();
		try {
			const prependedVisibleLines =
				await this.measurePrependedVisibleLines(chunk);

			this.replica.prependHistoryChunk(chunk, page.start_seq);
			this.terminal.reset();
			await writeToTerminal(this.terminal, this.replica.currentRenderedText());

			if (prependedVisibleLines > 0) {
				this.terminal.scrollToLine(previousViewportY + prependedVisibleLines);
			}

			for (const output of [
				...previousLiveBuffered,
				...this.replica.drainBufferedLiveOutput(),
			]) {
				this.appendLiveOutput(output.chunk, output.seq);
			}
			for (const output of this.replica.drainBufferedLiveOutput()) {
				this.appendLiveOutput(output.chunk, output.seq);
			}
		} finally {
			this.replica.endRedraw();
		}
	}

	private async measurePrependedVisibleLines(chunk: string) {
		if (this.host == null || this.terminal == null) {
			return estimateVisibleLineCount(
				chunk,
				this.lastReportedSize?.cols ?? 120,
			);
		}

		if (!hasAnsiOrControlSequences(chunk)) {
			return estimateVisibleLineCount(chunk, this.terminal.cols);
		}

		const probe = await this.ensureProbeTerminal();
		if (probe == null) {
			return estimateVisibleLineCount(chunk, this.terminal.cols);
		}

		try {
			probe.reset();
			await writeToTerminal(probe, chunk);
			return visibleLineCount(probe.buffer.active);
		} catch {
			return estimateVisibleLineCount(chunk, this.terminal.cols);
		}
	}
}
