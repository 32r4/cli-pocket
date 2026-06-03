import { beforeEach, describe, expect, it, vi } from "vitest";
import { TauriBridge } from "./TauriBridge";

const tauriMocks = vi.hoisted(() => {
	let resolveListen: ((unlisten: () => void) => void) | null = null;
	const invoke = vi.fn(async () => undefined);
	const listen = vi.fn(
		(_eventName: string, _handler: (event: { payload: unknown }) => void) =>
			new Promise<() => void>((resolve) => {
				resolveListen = resolve;
			}),
	);

	return {
		invoke,
		listen,
		resolveListen: (unlisten: () => void) => {
			if (resolveListen == null) {
				throw new Error("listen was not called");
			}
			resolveListen(unlisten);
		},
		reset: () => {
			resolveListen = null;
			invoke.mockClear();
			listen.mockClear();
		},
	};
});

vi.mock("@tauri-apps/api/core", () => ({
	invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
	listen: tauriMocks.listen,
}));

describe("TauriBridge", () => {
	beforeEach(() => {
		tauriMocks.reset();
	});

	it("waits for the event listener before starting a session", async () => {
		const bridge = new TauriBridge({ embeddedDaemon: false });

		const connectPromise = bridge.sessionFactory.connect({
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:17842/session",
		});
		await Promise.resolve();

		expect(tauriMocks.listen).toHaveBeenCalledTimes(1);
		expect(tauriMocks.invoke).not.toHaveBeenCalled();

		tauriMocks.resolveListen(() => undefined);
		await connectPromise;

		expect(tauriMocks.invoke).toHaveBeenCalledWith("cli_pocket_connect", {
			config: {
				kind: "direct",
				endpointUrl: "ws://127.0.0.1:17842/session",
			},
			eventChannel: expect.stringMatching(/^cli_pocket:event:/),
		});
	});
});
