import { describe, expect, it, vi } from "vitest";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { TerminalSessionActor } from "./terminalSessionActor";
import { TerminalSessionRegistry } from "./terminalSessionRegistry";

describe("TerminalSessionRegistry", () => {
	it("detaches the previous actor when activating a new terminal", () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "t1",
				attached_clients: 1,
			},
			{
				terminal: "t2",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 2,
				label: "t2",
				attached_clients: 1,
			},
		]);
		workspaceState.getState().setActiveSessionId("t1");
		const registry = new TerminalSessionRegistry({
			controller: {
				setActiveTerminal: vi.fn(),
				appendActiveOutput: vi.fn(),
				renderSnapshotWithRange: vi.fn(),
				prependHistoryPage: vi.fn(async () => undefined),
				mount: vi.fn(async () => undefined),
				unmount: vi.fn(),
			} as never,
			workspaceState,
			session: () => null,
			onInlineError: vi.fn(),
		});
		const actorSpy = vi.spyOn(
			TerminalSessionActor.prototype,
			"activateTerminal",
		);
		const detachSpy = vi.spyOn(TerminalSessionActor.prototype, "detach");

		registry.connect(1);
		workspaceState.getState().setActiveSessionId("t2");

		expect(actorSpy).toHaveBeenCalledWith(1);
		expect(detachSpy).toHaveBeenCalledTimes(1);

		actorSpy.mockRestore();
		detachSpy.mockRestore();
	});

	it("routes active history and resize commands to the active actor", () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "t1",
				attached_clients: 1,
			},
		]);
		workspaceState.getState().setActiveSessionId("t1");
		const registry = new TerminalSessionRegistry({
			controller: {
				setActiveTerminal: vi.fn(),
				appendActiveOutput: vi.fn(),
				renderSnapshotWithRange: vi.fn(),
				prependHistoryPage: vi.fn(async () => undefined),
				mount: vi.fn(async () => undefined),
				unmount: vi.fn(),
			} as never,
			workspaceState,
			session: () => null,
			onInlineError: vi.fn(),
		});
		const loadHistorySpy = vi.spyOn(
			TerminalSessionActor.prototype,
			"loadOlderHistory",
		);
		const resizeSpy = vi.spyOn(TerminalSessionActor.prototype, "resize");

		registry.connect(1);
		registry.loadOlderHistoryActive();
		registry.resizeActive(120, 40);

		expect(loadHistorySpy).toHaveBeenCalledTimes(1);
		expect(resizeSpy).toHaveBeenCalledWith(120, 40);

		loadHistorySpy.mockRestore();
		resizeSpy.mockRestore();
	});

	it("does not re-attach on terminal list refresh after connection", () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "t1",
				attached_clients: 1,
			},
		]);
		workspaceState.getState().setActiveSessionId("t1");
		const registry = new TerminalSessionRegistry({
			controller: {
				setActiveTerminal: vi.fn(),
				appendActiveOutput: vi.fn(),
				renderSnapshotWithRange: vi.fn(),
				prependHistoryPage: vi.fn(async () => undefined),
				mount: vi.fn(async () => undefined),
				unmount: vi.fn(),
			} as never,
			workspaceState,
			session: () => null,
			onInlineError: vi.fn(),
		});
		const actorSpy = vi.spyOn(
			TerminalSessionActor.prototype,
			"activateTerminal",
		);

		registry.connect(1);
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 120,
				rows: 36,
				created_at_unix_ms: 1,
				label: "t1",
				attached_clients: 1,
			},
		]);

		expect(actorSpy).toHaveBeenCalledTimes(1);

		actorSpy.mockRestore();
	});
});
