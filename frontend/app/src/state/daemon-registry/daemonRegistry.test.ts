import { describe, expect, it } from "vitest";
import { createDaemonRegistryStore } from "./daemonRegistry";

describe("daemon registry", () => {
	it("adds and selects a daemon", () => {
		const store = createDaemonRegistryStore();
		store.getState().upsertDaemon({
			id: "host-1",
			label: "Local host",
			serverPublicHex: "abcd",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "relay",
			hostId: "123e4567-e89b-12d3-a456-426614174000",
			relayUrl: "wss://relay.example/ws/client?host=123",
			relayPskHex: "11".repeat(32),
		});

		store.getState().selectDaemon("host-1");

		expect(store.getState().selectedDaemonId).toBe("host-1");
		expect(store.getState().daemons).toHaveLength(1);
	});

	it("updates a daemon label", () => {
		const store = createDaemonRegistryStore();
		store.getState().upsertDaemon({
			id: "host-1",
			label: "old",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7842/session",
		});

		store.getState().updateDaemonLabel("host-1", "new-name");

		expect(store.getState().daemons[0]?.label).toBe("new-name");
	});
});
