import { describe, expect, it } from "vitest";
import { createWorkspaceStore } from "./workspaceState";

describe("workspace state", () => {
	it("tracks terminal lifecycle", () => {
		const store = createWorkspaceStore();
		store
			.getState()
			.openTerminal({ id: "term-1", title: "shell", status: "connecting" });
		store.getState().markTerminalReady("term-1");

		expect(store.getState().activeTerminalId).toBe("term-1");
		expect(store.getState().terminals[0]?.status).toBe("ready");
	});
});
