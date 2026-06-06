import { afterEach, describe, expect, it, vi } from "vitest";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { TerminalSessionActor } from "./terminalSessionActor";

function deferred<T>() {
	let resolve!: (value: T | PromiseLike<T>) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((nextResolve, nextReject) => {
		resolve = nextResolve;
		reject = nextReject;
	});
	return { promise, resolve, reject };
}

function makeSnapshot(terminalId: string, endSeq: number, text = "") {
	const startSeq = endSeq - text.length;
	return {
		stream_id: 1,
		info: {
			terminal: terminalId,
			cols: 80,
			rows: 24,
			created_at_unix_ms: 1,
			label: terminalId,
			attached_clients: 1,
		},
		start_seq: startSeq,
		end_seq: endSeq,
		render_bytes_b64: btoa(text),
		has_more_history: startSeq > 0,
	};
}

function makeHistoryPage(
	terminalId: string,
	startSeq: number,
	endSeq: number,
	text: string,
) {
	return {
		terminal_id: terminalId,
		start_seq: startSeq,
		end_seq: endSeq,
		bytes_b64: btoa(text),
		has_more: startSeq > 0,
	};
}

function actorField<T>(actor: TerminalSessionActor, field: string) {
	return (actor as unknown as Record<string, T>)[field];
}

async function awaitPendingOpen(actor: TerminalSessionActor) {
	const pendingOpen = actorField<Promise<void> | null>(actor, "pendingOpen");
	if (pendingOpen != null) {
		await pendingOpen;
	}
	await Promise.resolve();
}

async function awaitPendingHistory(actor: TerminalSessionActor) {
	const pendingHistory = actorField<Promise<void> | null>(
		actor,
		"pendingHistory",
	);
	if (pendingHistory != null) {
		await pendingHistory;
	}
	await Promise.resolve();
}

function makeDeps(overrides?: {
	openTerminal?: (terminalId: string) => Promise<unknown>;
	readHistory?: (
		terminalId: string,
		before: number | null,
		maxBytes: number,
	) => Promise<{
		terminal_id: string;
		start_seq: number;
		end_seq: number;
		bytes_b64: string;
		has_more: boolean;
	}>;
}) {
	const workspaceState = createWorkspaceStore();
	workspaceState.getState().markConnected();
	workspaceState.getState().syncTerminalList([
		{
			terminal: "t1",
			cols: 80,
			rows: 24,
			created_at_unix_ms: 1,
			label: "shell",
			attached_clients: 1,
		},
	]);
	const controller = {
		setActiveTerminal: vi.fn(),
		renderSnapshot: vi.fn(),
		appendActiveOutput: vi.fn(),
		prependHistoryPage: vi.fn(async () => undefined),
		mount: vi.fn(async () => undefined),
		unmount: vi.fn(),
	};
	const session = {
		openTerminal:
			overrides?.openTerminal ??
			vi.fn(async (terminalId: string) => makeSnapshot(terminalId, 12)),
		readHistory:
			overrides?.readHistory ??
			vi.fn(async (terminalId: string, before: number | null) =>
				makeHistoryPage(
					terminalId,
					Math.max(0, (before ?? 0) - 4),
					before ?? 0,
					"latest",
				),
			),
		resize: vi.fn(async () => undefined),
	} as const;
	const onInlineError = vi.fn();

	return {
		workspaceState,
		controller,
		session,
		onInlineError,
		actor: new TerminalSessionActor({
			terminalId: "t1",
			controller: controller as never,
			workspaceState,
			session: () => session as never,
			onInlineError,
			onRuntimeStateChange: vi.fn(),
		}),
	};
}

describe("TerminalSessionActor", () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it("renders the attach snapshot immediately", async () => {
		const { actor, controller, session } = makeDeps({
			openTerminal: vi.fn(async (terminalId: string) =>
				makeSnapshot(terminalId, 12, "latest"),
			),
		});

		actor.activateTerminal(1);
		await awaitPendingOpen(actor);

		expect(session.openTerminal).toHaveBeenCalledTimes(1);
		expect(session.readHistory).not.toHaveBeenCalled();
		expect(controller.renderSnapshot).toHaveBeenCalledWith("t1", "latest", 6);
		expect(actor.getRuntimeState()).toEqual({
			phase: "ready",
			error: null,
		});
	});

	it("loads older history pages after the attach snapshot is rendered", async () => {
		const older = deferred<{
			terminal_id: string;
			start_seq: number;
			end_seq: number;
			bytes_b64: string;
		}>();
		const readHistory = vi
			.fn()
			.mockImplementationOnce(async () => older.promise);
		const { actor, controller } = makeDeps({
			openTerminal: vi.fn(async (terminalId: string) =>
				makeSnapshot(terminalId, 12, "latest"),
			),
			readHistory,
		});

		actor.activateTerminal(1);
		await awaitPendingOpen(actor);

		actor.loadOlderHistory();
		older.resolve(makeHistoryPage("t1", 0, 6, "older"));
		await awaitPendingHistory(actor);

		expect(readHistory).toHaveBeenCalledWith("t1", 6, 32 * 1024);
		expect(controller.prependHistoryPage).toHaveBeenCalledTimes(1);
		expect(controller.prependHistoryPage).toHaveBeenCalledWith(
			expect.objectContaining({
				terminal_id: "t1",
				start_seq: 0,
				end_seq: 6,
				bytes_b64: btoa("older"),
			}),
		);
	});

	it("fails the open after 10 seconds", async () => {
		vi.useFakeTimers();
		const open = deferred<unknown>();
		const { actor, controller, onInlineError } = makeDeps({
			openTerminal: vi.fn(async () => open.promise),
		});

		actor.activateTerminal(1);
		await vi.advanceTimersByTimeAsync(10_000);
		await awaitPendingOpen(actor);

		expect(controller.renderSnapshot).not.toHaveBeenCalled();
		expect(actor.getRuntimeState()).toEqual({
			phase: "failed",
			error: "terminal open timed out",
		});
		expect(onInlineError).not.toHaveBeenCalled();
	});
});
