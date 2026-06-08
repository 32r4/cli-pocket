import { describe, expect, it } from "vitest";
import type { TerminalInfoRecord } from "@/platform/bridge/types";
import { createWorkspaceStore } from "./workspaceState";

function terminal(id: string, createdAtUnixMs: number): TerminalInfoRecord {
	return {
		terminal: id,
		cols: 80,
		rows: 24,
		created_at_unix_ms: createdAtUnixMs,
		label: null,
		attached_clients: 1,
	};
}

describe("workspaceState", () => {
	it("does not auto-select terminals discovered by list sync", () => {
		const store = createWorkspaceStore();

		store.getState().syncTerminalList([terminal("t1", 1)]);

		expect(store.getState().terminals).toHaveLength(1);
		expect(store.getState().activeSessionId).toBeNull();
	});

	it("keeps the active terminal across list syncs", () => {
		const store = createWorkspaceStore();

		store.getState().syncTerminalList([terminal("t1", 1)]);
		store.getState().setActiveSessionId("t1");
		store.getState().syncTerminalList([terminal("t1", 1), terminal("t2", 2)]);

		expect(store.getState().activeSessionId).toBe("t1");
	});
});
