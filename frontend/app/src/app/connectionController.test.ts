import { describe, expect, it, vi } from "vitest";
import type {
	ConnectConfig,
	PlatformServices,
	SessionActor,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
import { createDaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import { createUiStateStore } from "@/state/ui/uiState";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { ConnectionController } from "./connectionController";

function makeActor(events: unknown[]): SessionActor {
	const iterator = events[Symbol.iterator]();

	return {
		events: () => ({
			[Symbol.asyncIterator]: () => ({
				next: async () => {
					const next = iterator.next();
					return next.done
						? { value: undefined, done: true }
						: { value: next.value, done: false };
				},
			}),
		}),
		refreshTerminals: vi.fn(async () => undefined),
		openTerminal: vi.fn(
			async () =>
				({
					info: {
						terminal: "t1",
						cols: 80,
						rows: 24,
						created_at_unix_ms: 1,
						label: null,
						attached_clients: 1,
					},
					snapshot_bytes_b64: "",
				}) as TerminalSnapshotRecord,
		),
		createTerminal: vi.fn(async () => null),
		sendInput: vi.fn(async () => undefined),
		resize: vi.fn(async () => undefined),
		kill: vi.fn(async () => undefined),
		close: vi.fn(async () => undefined),
	};
}

function makeServices(actor: SessionActor): PlatformServices {
	return {
		sessionFactory: {
			connect: vi.fn(async (_config: ConnectConfig) => actor),
		},
		registry: {
			load: vi.fn(async () => ({
				version: 1 as const,
				daemons: [],
				selectedDaemonId: null,
			})),
			save: vi.fn(async () => undefined),
			exportIdentity: vi.fn(async () => new Uint8Array()),
			importIdentity: vi.fn(async () => undefined),
		},
		host: null,
	};
}

describe("ConnectionController", () => {
	it("owns connect and consumes session actor events", async () => {
		const actor = makeActor([
			{ kind: "Connecting" },
			{
				kind: "Connected",
				server_label: "server-a",
			},
			{
				kind: "TerminalList",
				terminals: [
					{
						terminal: "t1",
						cols: 80,
						rows: 24,
						created_at_unix_ms: 1,
						label: "shell",
						attached_clients: 1,
					},
				],
			},
		]);
		const services = makeServices(actor);
		const daemonRegistry = createDaemonRegistryStore();
		const uiState = createUiStateStore();
		const workspaceState = createWorkspaceStore();
		daemonRegistry.hydratePersistedState({
			version: 1,
			daemons: [
				{
					id: "server-a",
					label: "server-a",
					kind: "direct",
					endpointUrl: "ws://127.0.0.1:9999",
					resumeTokenHex: null,
					lastConnectedAt: null,
				},
			],
			selectedDaemonId: "server-a",
		});
		uiState.getState().setSelectedServerId("server-a");

		const controller = new ConnectionController({
			services,
			daemonRegistry,
			uiState,
			workspaceState,
			onInlineError: vi.fn(),
			onConnectionReset: vi.fn(),
			onTerminalOutput: vi.fn(),
			onTerminalRemoved: vi.fn(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		await controller.connectServer(server);
		await new Promise((resolve) => setTimeout(resolve, 0));

		expect(services.sessionFactory.connect).toHaveBeenCalledTimes(1);
		expect(workspaceState.getState().connectionState).toBe("connected");
		expect(workspaceState.getState().activeConnectionServerId).toBe("server-a");
		expect(workspaceState.getState().terminals).toHaveLength(1);
		expect(workspaceState.getState().terminals[0]?.id).toBe("t1");
	});
});
