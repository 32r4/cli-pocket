import { describe, expect, it, vi } from "vitest";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { SessionController } from "./SessionController";

describe("SessionController", () => {
	it("opens a terminal after connect succeeds", async () => {
		const bridge = {
			connect: vi.fn().mockResolvedValue(undefined),
			events: vi.fn().mockReturnValue((async function* () {})()),
			createTerminal: vi.fn().mockResolvedValue(undefined),
			sendInput: vi.fn(),
			resize: vi.fn(),
			kill: vi.fn(),
			exportIdentity: vi.fn(),
			importIdentity: vi.fn(),
			close: vi.fn(),
		};

		const workspace = createWorkspaceStore();
		const controller = new SessionController(bridge, workspace);

		await controller.connectAndCreate({
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7842",
			serverPublicHex: "abcd",
		});

		expect(bridge.connect).toHaveBeenCalledTimes(1);
		expect(bridge.createTerminal).toHaveBeenCalledTimes(1);
	});
});
