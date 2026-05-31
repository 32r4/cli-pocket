import type { ThemeName } from "@/state/ui/uiState";

interface TerminalAddonLike {
	activate: (terminal: unknown) => void;
	dispose: () => void;
}

interface TerminalLike {
	cols: number;
	rows: number;
	open: (node: HTMLElement) => void;
	write: (data: string) => void;
	clear: () => void;
	focus: () => void;
	loadAddon: (addon: TerminalAddonLike) => void;
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

interface TerminalControllerOptions {
	onInput: (terminalId: string, data: string) => void;
	onResize: (terminalId: string, cols: number, rows: number) => void;
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

export class TerminalController {
	private onInput: TerminalControllerOptions["onInput"];
	private onResize: TerminalControllerOptions["onResize"];
	private activeTerminalId: string | null = null;
	private host: HTMLElement | null = null;
	private terminal: TerminalLike | null = null;
	private fitAddon: FitAddonLike | null = null;
	private dataSubscription: { dispose: () => void } | null = null;
	private removePointerFocusListener: (() => void) | null = null;
	private removeWindowResizeListener: (() => void) | null = null;
	private lastReportedSize: { cols: number; rows: number } | null = null;
	private initPromise: Promise<void> | null = null;

	constructor({ onInput, onResize }: TerminalControllerOptions) {
		this.onInput = onInput;
		this.onResize = onResize;
	}

	setHandlers({ onInput, onResize }: TerminalControllerOptions) {
		this.onInput = onInput;
		this.onResize = onResize;
	}

	reset() {
		this.activeTerminalId = null;
		if (this.terminal !== null) {
			this.terminal.clear();
		}
	}

	setTheme(_theme: ThemeName) {
		if (this.terminal?.optionsService?.options != null) {
			this.terminal.optionsService.options.theme = terminalTheme();
		}
	}

	setActiveTerminal(terminalId: string | null) {
		this.activeTerminalId = terminalId;
	}

	renderSnapshot(terminalId: string, snapshot: string) {
		if (this.terminal === null || this.activeTerminalId !== terminalId) {
			return;
		}

		this.terminal.clear();
		if (snapshot.length > 0) {
			this.terminal.write(snapshot);
		}
		this.fitToViewport();
	}

	appendActiveOutput(terminalId: string, chunk: string) {
		if (
			this.terminal === null ||
			this.activeTerminalId !== terminalId ||
			chunk.length === 0
		) {
			return;
		}

		this.terminal.write(chunk);
	}

	removeTerminal(terminalId: string) {
		if (this.activeTerminalId === terminalId) {
			this.activeTerminalId = null;
			this.terminal?.clear();
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
		if (this.host === null) {
			return;
		}
		if (this.terminal !== null) {
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

	private async createTerminal() {
		if (this.host === null) {
			return;
		}

		const modules = (await Promise.all([
			import("@xterm/xterm"),
			import("@xterm/addon-fit"),
		])) as unknown as [
			{ Terminal: TerminalModules["Terminal"] },
			{ FitAddon: TerminalModules["FitAddon"] },
		];
		const [{ Terminal }, { FitAddon }] = modules;

		if (this.host === null) {
			return;
		}

		const terminal = new Terminal({
			cols: 120,
			rows: 32,
			theme: terminalTheme(),
		});
		const fitAddon = new FitAddon();

		terminal.loadAddon(fitAddon);
		terminal.open(this.host);
		this.terminal = terminal;
		this.fitAddon = fitAddon;
		this.dataSubscription = terminal.onData((data: string) => {
			if (this.activeTerminalId == null) {
				return;
			}
			this.onInput(this.activeTerminalId, data);
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
	}

	private detachTerminal() {
		this.removePointerFocusListener?.();
		this.removePointerFocusListener = null;
		this.removeWindowResizeListener?.();
		this.removeWindowResizeListener = null;
		this.dataSubscription?.dispose();
		this.dataSubscription = null;
		this.terminal?.dispose();
		this.terminal = null;
		this.fitAddon = null;
		this.lastReportedSize = null;
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
}
