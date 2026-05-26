import { describe, expect, it } from "vitest";
import { createDaemonRegistryStore } from "./daemonRegistry";

describe("daemon registry", () => {
	it("adds and selects a daemon", () => {
		const store = createDaemonRegistryStore();
		store.getState().upsertDaemon({
			id: "host-1",
			label: "Local host",
			endpointUrl: "ws://127.0.0.1:7842",
			serverPublicHex: "abcd",
			resumeTokenHex: null,
			lastConnectedAt: null,
		});

		store.getState().selectDaemon("host-1");

		expect(store.getState().selectedDaemonId).toBe("host-1");
		expect(store.getState().daemons).toHaveLength(1);
	});
});
