import { describe, expect, it, vi } from "vitest";
import { TerminalController } from "./terminalController";

function controllerField<T>(controller: TerminalController, field: string) {
	return (controller as unknown as Record<string, T>)[field];
}

class MockTerminal {
	static instances: MockTerminal[] = [];
	cols = 10;
	rows = 4;
	options?: unknown;
	refreshCalls: Array<[number, number]> = [];
	focusCalls = 0;
	writes: string[] = [];
	scrollToLineCalls: number[] = [];
	scrollListener: ((position: number) => void) | null = null;
	buffer = {
		active: {
			viewportY: 0,
			baseY: 0,
			length: 0,
			getLine: (_y: number) => undefined,
		},
	};
	optionsService = {
		options: {},
	};

	constructor(options?: unknown) {
		this.options = options;
		MockTerminal.instances.push(this);
	}

	open(_node: HTMLElement) {}
	loadAddon(_addon: unknown) {}
	refresh(start: number, end: number) {
		this.refreshCalls.push([start, end]);
	}
	focus() {
		this.focusCalls += 1;
	}
	clear() {
		this.writes = [];
		this.buffer.active.length = 0;
		this.buffer.active.baseY = 0;
		this.buffer.active.viewportY = 0;
	}
	reset() {
		this.clear();
	}
	write(data: string, callback?: () => void) {
		this.writes.push(data);
		this.buffer.active.length += Math.max(1, data.split("\n").length);
		this.buffer.active.baseY = Math.max(
			0,
			this.buffer.active.length - this.rows,
		);
		callback?.();
	}
	onData(_listener: (data: string) => void) {
		return { dispose: () => undefined };
	}
	onScroll(listener: (position: number) => void) {
		this.scrollListener = listener;
		return { dispose: () => undefined };
	}
	scrollToLine(line: number) {
		this.scrollToLineCalls.push(line);
		this.buffer.active.viewportY = line;
	}
	dispose() {}
}

class MockFitAddon {
	fit() {}
	activate() {}
	dispose() {}
}

vi.mock("@xterm/xterm", () => {
	return {
		Terminal: MockTerminal,
	};
});

vi.mock("@xterm/addon-fit", () => {
	return {
		FitAddon: MockFitAddon,
	};
});

describe("TerminalController", () => {
	it("creates xterm with a large scrollback buffer", async () => {
		MockTerminal.instances = [];
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);

		expect(MockTerminal.instances[0]?.options).toMatchObject({
			scrollback: 100_000,
		});
	});

	it("creates xterm with the configured font size", async () => {
		MockTerminal.instances = [];
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
			terminalFontSize: 18,
		});
		const host = document.createElement("div");

		await controller.mount(host);

		expect(MockTerminal.instances[0]?.options).toMatchObject({
			fontFamily: '"IBM Plex Mono", "Cascadia Code", monospace',
			fontSize: 18,
			lineHeight: 1.05,
		});
	});

	it("applies a compact font size offset on mobile", async () => {
		MockTerminal.instances = [];
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
			terminalFontSize: 18,
			compactMode: true,
		});
		const host = document.createElement("div");

		await controller.mount(host);

		expect(MockTerminal.instances[0]?.options).toMatchObject({
			fontSize: 16,
			lineHeight: 1,
		});
	});

	it("updates the terminal theme through the public options API", async () => {
		MockTerminal.instances = [];
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);
		controller.setTheme("light");

		const terminal = MockTerminal.instances[0];
		expect(terminal?.options).toMatchObject({
			theme: {
				background: "#f8fbfb",
				foreground: "#0f1d20",
				cursor: "#0b3a63",
				selectionBackground: "rgba(37, 79, 113, 0.32)",
				black: "#0f1d20",
				white: "#435255",
				brightBlack: "#3a494c",
				brightWhite: "#172326",
			},
		});
		expect(terminal?.refreshCalls).toEqual([[0, 3]]);
	});

	it("updates the terminal font size through the public options API", async () => {
		MockTerminal.instances = [];
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);
		controller.setTerminalFontSize(17);

		const terminal = MockTerminal.instances[0];
		expect(terminal?.options).toMatchObject({
			fontSize: 17,
		});
		expect(terminal?.refreshCalls).toEqual([]);
	});

	it("tracks loaded seq range from the snapshot", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);
		controller.setActiveTerminal("t1");
		controller.renderSnapshot("t1", "hello", 10);

		expect(controller.getLoadedRange()).toEqual({
			startSeq: 10,
			endSeq: 15,
		});
	});

	it("focuses the terminal when the active terminal changes", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);
		const terminal = controllerField<MockTerminal | null>(
			controller,
			"terminal",
		);
		expect(terminal).not.toBeNull();
		if (terminal == null) {
			throw new Error("expected terminal to mount");
		}

		const initialFocusCalls = terminal.focusCalls;
		controller.setActiveTerminal("t1");

		await new Promise<void>((resolve) => {
			window.requestAnimationFrame(() => {
				resolve();
			});
		});

		expect(terminal.focusCalls).toBeGreaterThan(initialFocusCalls);
	});

	it("replays a snapshot that arrives before the terminal mounts", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		controller.setActiveTerminal("t1");
		controller.renderSnapshot("t1", "hello", 10);
		await controller.mount(host);

		const terminal = controllerField<MockTerminal | null>(
			controller,
			"terminal",
		);
		expect(terminal?.writes).toEqual(["hello"]);
		expect(controller.getLoadedRange()).toEqual({
			startSeq: 10,
			endSeq: 15,
		});
	});

	it("replays live output buffered before the terminal mounts", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		controller.setActiveTerminal("t1");
		controller.renderSnapshot("t1", "hello", 10);
		controller.appendActiveOutput("t1", "tail", 19);
		await controller.mount(host);

		const terminal = controllerField<MockTerminal | null>(
			controller,
			"terminal",
		);
		expect(terminal?.writes).toEqual(["hello", "tail"]);
		expect(controller.currentRenderedText()).toBe("hellotail");
		expect(controller.getLoadedRange()).toEqual({
			startSeq: 10,
			endSeq: 19,
		});
	});

	it("preserves snapshot and detached live output across unmount and remount", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const firstHost = document.createElement("div");
		const secondHost = document.createElement("div");

		await controller.mount(firstHost);
		controller.setActiveTerminal("t1");
		controller.renderSnapshot("t1", "hello", 10);
		controller.unmount();
		controller.appendActiveOutput("t1", "tail", 19);

		await controller.mount(secondHost);

		const terminal = controllerField<MockTerminal | null>(
			controller,
			"terminal",
		);
		expect(terminal?.writes).toEqual(["hello", "tail"]);
		expect(controller.currentRenderedText()).toBe("hellotail");
		expect(controller.getLoadedRange()).toEqual({
			startSeq: 10,
			endSeq: 19,
		});
	});

	it("buffers live output while redrawing history and replays it after redraw", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);
		controller.setActiveTerminal("t1");
		controller.renderSnapshot("t1", "live\n", 5);

		const terminal = controllerField<MockTerminal | null>(
			controller,
			"terminal",
		);
		expect(terminal).not.toBeNull();
		if (terminal == null) {
			throw new Error("expected terminal to mount");
		}
		terminal.buffer.active.viewportY = 0;
		const loadPromise = controller.prependHistoryPage({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 5,
			bytes_b64: btoa("old\n"),
			has_more: false,
		});
		controller.appendActiveOutput("t1", "tail", 14);
		await loadPromise;

		expect(controller.currentRenderedText()).toBe("old\nlive\ntail");
		expect(controller.getLoadedRange()).toEqual({
			startSeq: 0,
			endSeq: 14,
		});
	});

	it("uses the string estimator for plain-text history chunks", async () => {
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory: vi.fn(),
		});
		const host = document.createElement("div");

		await controller.mount(host);
		controller.setActiveTerminal("t1");
		controller.renderSnapshot("t1", "live\n", 5);

		const ensureProbeTerminal = vi.spyOn(
			controller as unknown as {
				ensureProbeTerminal: () => Promise<unknown>;
			},
			"ensureProbeTerminal",
		);
		await controller.prependHistoryPage({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 5,
			bytes_b64: btoa("old\n"),
			has_more: false,
		});

		expect(ensureProbeTerminal).not.toHaveBeenCalled();
	});

	it("requests older history when the viewport scrolls to the top", async () => {
		const onLoadOlderHistory = vi.fn();
		const controller = new TerminalController({
			onInput: vi.fn(),
			onResize: vi.fn(),
			onLoadOlderHistory,
		});
		const host = document.createElement("div");

		await controller.mount(host);

		const terminal = controllerField<MockTerminal | null>(
			controller,
			"terminal",
		);
		expect(terminal).not.toBeNull();
		if (terminal == null || terminal.scrollListener == null) {
			throw new Error("expected scroll listener");
		}

		terminal.scrollListener(0);

		expect(onLoadOlderHistory).toHaveBeenCalledTimes(1);
	});
});
