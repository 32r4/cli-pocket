import { beforeEach, describe, expect, it } from "vitest";
import { createDaemonRegistryStore } from "./daemonRegistry";

describe("daemon registry", () => {
	beforeEach(() => {
		localStorage.clear();
	});

	it("adds and selects a daemon", () => {
		const store = createDaemonRegistryStore();
		store.getState().upsertDaemon({
			id: "server-1",
			label: "Local server",
			serverPublicHex: "abcd",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "relay",
			serverId: "123e4567-e89b-12d3-a456-426614174000",
			relayUrl: "wss://relay.example/ws/client?server=123",
			relayPskHex: "11".repeat(32),
		});

		store.getState().selectDaemon("server-1");

		expect(store.getState().selectedDaemonId).toBe("server-1");
		expect(store.getState().daemons).toHaveLength(1);
	});

	it("updates a daemon label", () => {
		const store = createDaemonRegistryStore();
		store.getState().upsertDaemon({
			id: "server-1",
			label: "old",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7842/session",
		});

		store.getState().updateDaemonLabel("server-1", "new-name");

		expect(store.getState().daemons[0]?.label).toBe("new-name");
	});

	it("restores persisted daemons and selection", () => {
		localStorage.setItem(
			"cli-pocket/daemon-registry/v1",
			JSON.stringify({
				version: 1,
				selectedDaemonId: "server-2",
				daemons: [
					{
						id: "server-2",
						label: "Saved server",
						resumeTokenHex: null,
						lastConnectedAt: null,
						kind: "direct",
						endpointUrl: "ws://127.0.0.1:7842/session",
					},
				],
			}),
		);

		const store = createDaemonRegistryStore();

		expect(store.getState().selectedDaemonId).toBe("server-2");
		expect(store.getState().daemons[0]?.label).toBe("Saved server");
	});
});
