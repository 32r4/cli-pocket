import { describe, expect, it, vi } from "vitest";
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

function makeSnapshot(
	terminalId: string,
	text: string,
	startSeq = 0,
	endSeq?: number,
) {
	return {
		info: {
			terminal: terminalId,
			cols: 80,
			rows: 24,
			created_at_unix_ms: 1,
			label: terminalId,
			attached_clients: 1,
		},
		start_seq: startSeq,
		end_seq: endSeq ?? startSeq + text.length,
		render_prefix_b64: btoa(""),
		snapshot_bytes_b64: btoa(text),
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
	activateTerminal?: (terminalId: string) => Promise<unknown>;
	readHistory?: (
		terminalId: string,
		before: number | null,
		maxBytes: number,
	) => Promise<{
		terminal_id: string;
		start_seq: number;
		end_seq: number;
		bytes_b64: string;
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
		renderSnapshotWithRange: vi.fn(),
		appendActiveOutput: vi.fn(),
		prependHistoryPage: vi.fn(async () => undefined),
		mount: vi.fn(async () => undefined),
		unmount: vi.fn(),
	};
	const session = {
		activateTerminal:
			overrides?.activateTerminal ??
			vi.fn(async (terminalId: string) => makeSnapshot(terminalId, "snap")),
		readHistory:
			overrides?.readHistory ??
			vi.fn(async (terminalId: string, before: number | null) => ({
				terminal_id: terminalId,
				start_seq: Math.max(0, (before ?? 0) - 5),
				end_seq: before ?? 0,
				bytes_b64: btoa("older"),
			})),
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
		}),
	};
}

describe("TerminalSessionActor", () => {
	it("drops a late open result after a newer activation", async () => {
		const firstOpen = deferred<unknown>();
		const secondOpen = deferred<unknown>();
		const activateTerminal = vi
			.fn<(terminalId: string) => Promise<unknown>>()
			.mockImplementationOnce(async () => firstOpen.promise)
			.mockImplementationOnce(async () => secondOpen.promise);
		const { actor, controller, workspaceState } = makeDeps({
			activateTerminal,
		});

		actor.activateTerminal(1);
		actor.activateTerminal(1);
		secondOpen.resolve(makeSnapshot("t1", "second"));
		await awaitPendingOpen(actor);
		firstOpen.resolve(makeSnapshot("t1", "first"));
		await firstOpen.promise;
		await Promise.resolve();

		expect(activateTerminal).toHaveBeenCalledTimes(2);
		expect(controller.renderSnapshotWithRange).toHaveBeenCalledTimes(1);
		expect(controller.renderSnapshotWithRange).toHaveBeenCalledWith(
			"t1",
			"second",
			0,
		);
		expect(workspaceState.getState().terminals[0]?.status).toBe("ready");
	});

	it("ignores duplicate history loads while one is in flight", async () => {
		const history = deferred<{
			terminal_id: string;
			start_seq: number;
			end_seq: number;
			bytes_b64: string;
		}>();
		const readHistory = vi
			.fn()
			.mockImplementationOnce(async () => ({
				terminal_id: "t1",
				start_seq: 4,
				end_seq: 4,
				bytes_b64: "",
			}))
			.mockImplementationOnce(async () => history.promise);
		const { actor, controller } = makeDeps({
			activateTerminal: vi.fn(async (terminalId: string) =>
				makeSnapshot(terminalId, "snap", 4, 8),
			),
			readHistory,
		});

		actor.activateTerminal(1);
		await awaitPendingOpen(actor);
		actor.loadOlderHistory();
		actor.loadOlderHistory();

		expect(readHistory).toHaveBeenCalledTimes(2);

		history.resolve({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 4,
			bytes_b64: btoa("old"),
		});
		await awaitPendingHistory(actor);

		expect(controller.prependHistoryPage).toHaveBeenCalledTimes(1);
	});

	it("buffers output during history redraw and replays it in seq order", async () => {
		const history = deferred<{
			terminal_id: string;
			start_seq: number;
			end_seq: number;
			bytes_b64: string;
		}>();
		const readHistory = vi
			.fn()
			.mockImplementationOnce(async () => ({
				terminal_id: "t1",
				start_seq: 4,
				end_seq: 4,
				bytes_b64: "",
			}))
			.mockImplementationOnce(async () => history.promise);
		const { actor, controller } = makeDeps({
			activateTerminal: vi.fn(async (terminalId: string) =>
				makeSnapshot(terminalId, "snap", 4, 8),
			),
			readHistory,
		});

		actor.activateTerminal(1);
		await awaitPendingOpen(actor);
		actor.loadOlderHistory();
		actor.applyOutput("t1", 9, "b", 1);
		actor.applyOutput("t1", 8, "a", 1);

		history.resolve({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 4,
			bytes_b64: btoa("old"),
		});
		await awaitPendingHistory(actor);

		expect(controller.appendActiveOutput).toHaveBeenNthCalledWith(
			1,
			"t1",
			"a",
			8,
		);
		expect(controller.appendActiveOutput).toHaveBeenNthCalledWith(
			2,
			"t1",
			"b",
			9,
		);
	});

	it("disconnect clears in-flight history without replaying stale completion", async () => {
		const history = deferred<{
			terminal_id: string;
			start_seq: number;
			end_seq: number;
			bytes_b64: string;
		}>();
		const readHistory = vi
			.fn()
			.mockImplementationOnce(async () => ({
				terminal_id: "t1",
				start_seq: 4,
				end_seq: 4,
				bytes_b64: "",
			}))
			.mockImplementationOnce(async () => history.promise);
		const { actor, controller } = makeDeps({
			activateTerminal: vi.fn(async (terminalId: string) =>
				makeSnapshot(terminalId, "snap", 4, 8),
			),
			readHistory,
		});

		actor.activateTerminal(1);
		await awaitPendingOpen(actor);
		actor.loadOlderHistory();
		actor.disconnect(2);
		history.resolve({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 4,
			bytes_b64: btoa("old"),
		});
		await awaitPendingHistory(actor);

		expect(controller.prependHistoryPage).not.toHaveBeenCalled();
		expect(controller.appendActiveOutput).not.toHaveBeenCalled();
	});

	it("disconnect during open drops the late snapshot completion", async () => {
		const open = deferred<unknown>();
		const { actor, controller, onInlineError } = makeDeps({
			activateTerminal: vi.fn(async () => open.promise),
		});

		actor.activateTerminal(1);
		actor.disconnect(2);
		open.resolve(makeSnapshot("t1", "late"));
		await open.promise;
		await Promise.resolve();

		expect(controller.renderSnapshotWithRange).not.toHaveBeenCalled();
		expect(onInlineError).not.toHaveBeenCalled();
	});

	it("disconnect then reactivate drops stale open and renders only the reconnected open", async () => {
		const firstOpen = deferred<unknown>();
		const secondOpen = deferred<unknown>();
		const activateTerminal = vi
			.fn<(terminalId: string) => Promise<unknown>>()
			.mockImplementationOnce(async () => firstOpen.promise)
			.mockImplementationOnce(async () => secondOpen.promise);
		const { actor, controller, workspaceState } = makeDeps({
			activateTerminal,
		});

		actor.activateTerminal(1);
		actor.disconnect(2);
		actor.activateTerminal(3);

		firstOpen.resolve(makeSnapshot("t1", "stale"));
		await firstOpen.promise;
		secondOpen.resolve(makeSnapshot("t1", "fresh"));
		await awaitPendingOpen(actor);

		expect(controller.renderSnapshotWithRange).toHaveBeenCalledTimes(1);
		expect(controller.renderSnapshotWithRange).toHaveBeenCalledWith(
			"t1",
			"fresh",
			0,
		);
		expect(workspaceState.getState().terminals[0]?.status).toBe("ready");
	});

	it("keeps the last activation rendered across rapid out-of-order switches", async () => {
		const opens = new Map<
			string,
			Array<
				ReturnType<
					typeof deferred<{
						info: {
							terminal: string;
							cols: number;
							rows: number;
							created_at_unix_ms: number;
							label: string;
							attached_clients: number;
						};
						start_seq: number;
						end_seq: number;
						render_prefix_b64: string;
						snapshot_bytes_b64: string;
					}>
				>
			>
		>();
		const activateTerminal = vi.fn(async (terminalId: string) => {
			const pending = deferred<ReturnType<typeof makeSnapshot>>();
			const queued = opens.get(terminalId) ?? [];
			queued.push(pending);
			opens.set(terminalId, queued);
			return pending.promise;
		});
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "a",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "a",
				attached_clients: 1,
			},
			{
				terminal: "b",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 2,
				label: "b",
				attached_clients: 1,
			},
		]);
		const controller = {
			setActiveTerminal: vi.fn(),
			renderSnapshotWithRange: vi.fn(),
			appendActiveOutput: vi.fn(),
			prependHistoryPage: vi.fn(async () => undefined),
			mount: vi.fn(async () => undefined),
			unmount: vi.fn(),
		};
		const session = {
			activateTerminal,
			readHistory: vi.fn(async () => ({
				terminal_id: "a",
				start_seq: 0,
				end_seq: 0,
				bytes_b64: "",
			})),
			resize: vi.fn(async () => undefined),
		};
		const actorA = new TerminalSessionActor({
			terminalId: "a",
			controller: controller as never,
			workspaceState,
			session: () => session as never,
			onInlineError: vi.fn(),
		});
		const actorB = new TerminalSessionActor({
			terminalId: "b",
			controller: controller as never,
			workspaceState,
			session: () => session as never,
			onInlineError: vi.fn(),
		});

		const switchCount = 20;
		for (let index = 0; index < switchCount; index += 1) {
			if (index % 2 === 0) {
				actorA.activateTerminal(1);
				actorB.detach();
			} else {
				actorB.activateTerminal(1);
				actorA.detach();
			}
		}

		const finalTerminalId = (switchCount - 1) % 2 === 0 ? "a" : "b";
		const terminalIds = [...opens.keys()].sort().reverse();
		for (const terminalId of terminalIds) {
			const queued = opens.get(terminalId) ?? [];
			for (let index = queued.length - 1; index >= 0; index -= 1) {
				queued[index].resolve(
					makeSnapshot(terminalId, `${terminalId}-${index}`),
				);
				await queued[index].promise;
			}
		}
		await Promise.resolve();

		expect(controller.renderSnapshotWithRange).toHaveBeenCalledTimes(1);
		expect(controller.renderSnapshotWithRange).toHaveBeenCalledWith(
			finalTerminalId,
			expect.stringMatching(new RegExp(`^${finalTerminalId}-`)),
			0,
		);
	});
});
