import { describe, expect, it, vi } from "vitest";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { TerminalSessionActor } from "./terminalSessionActor";
import { TerminalSessionRegistry } from "./terminalSessionRegistry";

describe("TerminalSessionRegistry", () => {
	it("detaches the previous actor when activating a new terminal", () => {
		const workspaceState = createWorkspaceStore();
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

		registry.activateTerminal("t1", 1);
		registry.activateTerminal("t2", 1);

		expect(actorSpy).toHaveBeenCalledWith(1);
		expect(detachSpy).toHaveBeenCalledTimes(1);

		actorSpy.mockRestore();
		detachSpy.mockRestore();
	});

	it("routes active history and resize commands to the active actor", () => {
		const workspaceState = createWorkspaceStore();
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

		registry.activateTerminal("t1", 1);
		registry.loadOlderHistoryActive();
		registry.resizeActive(120, 40);

		expect(loadHistorySpy).toHaveBeenCalledTimes(1);
		expect(resizeSpy).toHaveBeenCalledWith(120, 40);

		loadHistorySpy.mockRestore();
		resizeSpy.mockRestore();
	});
});
