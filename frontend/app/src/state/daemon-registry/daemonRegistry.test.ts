import { describe, expect, it } from "vitest";
import { createDaemonRegistryStore } from "./daemonRegistry";

describe("daemon registry", () => {
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

	it("removes a daemon and falls back the selection", () => {
		const store = createDaemonRegistryStore();
		store.getState().upsertDaemon({
			id: "server-1",
			label: "one",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7842/session",
		});
		store.getState().upsertDaemon({
			id: "server-2",
			label: "two",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7843/session",
		});
		store.getState().selectDaemon("server-2");

		store.getState().removeDaemon("server-2");

		expect(store.getState().daemons.map((daemon) => daemon.id)).toEqual([
			"server-1",
		]);
		expect(store.getState().selectedDaemonId).toBe("server-1");
	});

	it("hydrates persisted daemons and selection", () => {
		const store = createDaemonRegistryStore();
		store.hydratePersistedState({
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
		});

		expect(store.getState().selectedDaemonId).toBe("server-2");
		expect(store.getState().daemons[0]?.label).toBe("Saved server");
	});

	it("does not immediately re-save hydrated state", () => {
		const saved: unknown[] = [];
		const store = createDaemonRegistryStore();
		store.setPersistence({
			load: async () => ({
				version: 1,
				selectedDaemonId: "native-server",
				daemons: [
					{
						id: "native-server",
						label: "Native",
						resumeTokenHex: null,
						lastConnectedAt: null,
						kind: "direct",
						endpointUrl: "ws://127.0.0.1:7842/session",
					},
				],
			}),
			save: async (state) => {
				saved.push(state);
			},
		});
		store.hydratePersistedState({
			version: 1,
			selectedDaemonId: "native-server",
			daemons: [
				{
					id: "native-server",
					label: "Native",
					resumeTokenHex: null,
					lastConnectedAt: null,
					kind: "direct",
					endpointUrl: "ws://127.0.0.1:7842/session",
				},
			],
		});

		expect(saved).toHaveLength(0);
		expect(store.getState().selectedDaemonId).toBe("native-server");
		expect(store.getState().daemons[0]?.label).toBe("Native");
	});

	it("saves updates through the configured persistence", () => {
		const saved: unknown[] = [];
		const store = createDaemonRegistryStore();
		store.setPersistence({
			load: async () => null,
			save: async (state) => {
				saved.push(state);
			},
		});

		store.getState().upsertDaemon({
			id: "server-1",
			label: "Local server",
			resumeTokenHex: null,
			lastConnectedAt: null,
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7842/session",
		});
		store.getState().selectDaemon("server-1");

		expect(saved).toEqual([
			{
				version: 1,
				selectedDaemonId: null,
				daemons: [
					{
						id: "server-1",
						label: "Local server",
						resumeTokenHex: null,
						lastConnectedAt: null,
						kind: "direct",
						endpointUrl: "ws://127.0.0.1:7842/session",
					},
				],
			},
			{
				version: 1,
				selectedDaemonId: "server-1",
				daemons: [
					{
						id: "server-1",
						label: "Local server",
						resumeTokenHex: null,
						lastConnectedAt: null,
						kind: "direct",
						endpointUrl: "ws://127.0.0.1:7842/session",
					},
				],
			},
		]);
	});
});
