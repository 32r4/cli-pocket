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
});
