import { waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TerminalController } from "@/features/terminals/terminalController";
import type { SessionActor } from "@/platform/bridge/types";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { TerminalSessionRegistry } from "./terminalSessionRegistry";

function makeController(): TerminalController {
	return {
		appendActiveOutput: vi.fn(),
		measureViewportSize: vi.fn(async () => null),
		mount: vi.fn(async () => undefined),
		prependHistoryPage: vi.fn(async () => undefined),
		removeTerminal: vi.fn(),
		renderSnapshot: vi.fn(),
		reset: vi.fn(),
		setActiveTerminal: vi.fn(),
		unmount: vi.fn(),
	} as unknown as TerminalController;
}

function makeSession(): SessionActor {
	return {
		events: () => ({
			async *[Symbol.asyncIterator]() {},
		}),
		refreshTerminals: vi.fn(async () => undefined),
		openTerminal: vi.fn(async () => {
			throw new Error("open failed");
		}),
		readHistory: vi.fn(async () => ({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 0,
			bytes_b64: "",
			has_more: false,
		})),
		createTerminal: vi.fn(async () => null),
		getServerConfig: vi.fn(async () => ({ scrollback_bytes: 4 * 1024 * 1024 })),
		setServerConfig: vi.fn(async (config) => config),
		sendInput: vi.fn(async () => undefined),
		resize: vi.fn(async () => undefined),
		kill: vi.fn(async () => undefined),
		close: vi.fn(async () => undefined),
	};
}

describe("TerminalSessionRegistry", () => {
	it("keeps terminal open failures local to the active terminal", async () => {
		const onInlineError = vi.fn();
		const registry = new TerminalSessionRegistry({
			controller: makeController(),
			workspaceState: createWorkspaceStore(),
			session: makeSession,
			onInlineError,
		});

		registry.setSelectedTerminal("t1");
		registry.connect(1);

		await waitFor(() => {
			expect(registry.activeRuntimeState()).toEqual({
				phase: "failed",
				error: "open failed",
			});
		});
		expect(onInlineError).not.toHaveBeenCalled();
	});
});
