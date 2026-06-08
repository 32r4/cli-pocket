import { waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type {
	TerminalRuntimeState,
	TerminalSessionRegistry,
} from "@/features/terminals/terminalSessionRegistry";
import type {
	ConnectConfig,
	PlatformServices,
	ServerConfigRecord,
	SessionActor,
	TerminalInfoRecord,
	TerminalOpenAckRecord,
} from "@/platform/bridge/types";
import { createDaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import { createUiStateStore } from "@/state/ui/uiState";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { ConnectionController } from "./connectionController";

function makeActor(
	events: unknown[],
	options?: { closeAfterEvents?: boolean },
) {
	const iterator = events[Symbol.iterator]();
	const refreshTerminals = vi.fn(async () => undefined);

	return {
		actor: {
			events: () => ({
				[Symbol.asyncIterator]: () => ({
					next: async () => {
						const next = iterator.next();
						if (!next.done) {
							return { value: next.value, done: false };
						}
						if (options?.closeAfterEvents === true) {
							return { value: undefined, done: true };
						}
						return await new Promise<IteratorResult<unknown>>(() => undefined);
					},
				}),
			}),
			refreshTerminals,
			openTerminal: vi.fn(
				async () =>
					({
						stream_id: 1,
						info: {
							terminal: "t1",
							cols: 80,
							rows: 24,
							created_at_unix_ms: 1,
							label: null,
							attached_clients: 1,
						},
						start_seq: 0,
						end_seq: 0,
						render_bytes_b64: "",
						has_more_history: false,
					}) as TerminalOpenAckRecord,
			),
			readHistory: vi.fn(async () => ({
				terminal_id: "t1",
				start_seq: 0,
				end_seq: 0,
				bytes_b64: "",
				has_more: false,
			})),
			createTerminal: vi.fn(
				async (): Promise<TerminalInfoRecord | null> => null,
			),
			getServerConfig: vi.fn(
				async () =>
					({
						scrollback_bytes: 4 * 1024 * 1024,
					}) satisfies ServerConfigRecord,
			),
			setServerConfig: vi.fn(async (config: ServerConfigRecord) => config),
			sendInput: vi.fn(async () => undefined),
			resize: vi.fn(async () => undefined),
			kill: vi.fn(async () => undefined),
			close: vi.fn(async () => undefined),
		} satisfies SessionActor,
		refreshTerminals,
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

function makeTerminalRegistry(): TerminalSessionRegistry {
	return {
		connect: vi.fn(),
		activeRuntimeState: vi.fn(() => null),
		applyOutput: vi.fn(),
		disconnect: vi.fn(),
		removeTerminal: vi.fn(),
		retryActive: vi.fn(),
		setSelectedTerminal: vi.fn(),
		dispose: vi.fn(),
		mountActive: vi.fn(async () => undefined),
		unmountActive: vi.fn(),
		resizeActive: vi.fn(),
		measureViewportSize: vi.fn(async () => null),
	} as unknown as TerminalSessionRegistry;
}

describe("ConnectionController", () => {
	it("owns connect and consumes session actor events", async () => {
		const { actor } = makeActor([
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
					{
						terminal: "t2",
						cols: 100,
						rows: 30,
						created_at_unix_ms: 2,
						label: "shell-2",
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
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
		expect(workspaceState.getState().terminals).toHaveLength(2);
		expect(workspaceState.getState().activeSessionId).toBeNull();
		expect(workspaceState.getState().terminals[1]?.id).toBe("t2");
	});

	it("creates a terminal when the connected server has none", async () => {
		const createdTerminal = {
			terminal: "t-new",
			cols: 120,
			rows: 36,
			created_at_unix_ms: 3,
			label: null,
			attached_clients: 1,
		};
		const { actor } = makeActor([
			{ kind: "Connecting" },
			{
				kind: "Connected",
				server_label: "server-a",
			},
			{
				kind: "TerminalList",
				terminals: [],
			},
		]);
		actor.createTerminal = vi.fn(
			async (): Promise<TerminalInfoRecord | null> => createdTerminal,
		);
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		await controller.connectServer(server);

		await waitFor(() => {
			expect(actor.createTerminal).toHaveBeenCalledWith({
				cols: 120,
				rows: 36,
			});
			expect(workspaceState.getState().activeSessionId).toBe("t-new");
		});
	});

	it("polls terminal list after connecting", async () => {
		vi.useFakeTimers();
		try {
			const { actor, refreshTerminals } = makeActor([
				{ kind: "Connecting" },
				{
					kind: "Connected",
					server_label: "server-a",
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
				onTerminalRemoved: vi.fn(),
				terminalRegistry: makeTerminalRegistry(),
			});

			const server = daemonRegistry.getState().daemons[0];
			expect(server).toBeDefined();
			if (server == null) {
				throw new Error("expected seeded daemon");
			}

			await controller.connectServer(server);
			for (
				let attempt = 0;
				attempt < 10 && refreshTerminals.mock.calls.length === 0;
				attempt += 1
			) {
				await Promise.resolve();
			}

			expect(refreshTerminals).toHaveBeenCalledTimes(1);

			await vi.advanceTimersByTimeAsync(1000);
			await Promise.resolve();
			expect(refreshTerminals).toHaveBeenCalledTimes(2);

			await vi.advanceTimersByTimeAsync(1000);
			await Promise.resolve();
			expect(refreshTerminals).toHaveBeenCalledTimes(3);
		} finally {
			vi.useRealTimers();
		}
	});

	it("pauses terminal polling while the active terminal is opening", async () => {
		vi.useFakeTimers();
		try {
			const { actor, refreshTerminals } = makeActor([
				{ kind: "Connecting" },
				{
					kind: "Connected",
					server_label: "server-a",
				},
			]);
			const services = makeServices(actor);
			const daemonRegistry = createDaemonRegistryStore();
			const uiState = createUiStateStore();
			const workspaceState = createWorkspaceStore();
			const terminalRegistry = makeTerminalRegistry();
			terminalRegistry.activeRuntimeState = vi
				.fn<() => TerminalRuntimeState | null>()
				.mockReturnValueOnce({
					phase: "opening",
					error: null,
				})
				.mockReturnValueOnce({
					phase: "opening",
					error: null,
				})
				.mockReturnValue({
					phase: "ready",
					error: null,
				});
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
				onTerminalRemoved: vi.fn(),
				terminalRegistry,
			});

			const server = daemonRegistry.getState().daemons[0];
			expect(server).toBeDefined();
			if (server == null) {
				throw new Error("expected seeded daemon");
			}

			await controller.connectServer(server);
			await Promise.resolve();
			expect(refreshTerminals).toHaveBeenCalledTimes(0);

			await vi.advanceTimersByTimeAsync(1000);
			await Promise.resolve();
			expect(refreshTerminals).toHaveBeenCalledTimes(0);

			await vi.advanceTimersByTimeAsync(1000);
			await Promise.resolve();
			expect(refreshTerminals).toHaveBeenCalledTimes(1);
		} finally {
			vi.useRealTimers();
		}
	});

	it("does not start a second connect while the same server is already connecting", async () => {
		let releaseFirstConnect: (() => void) | undefined;
		const actor = makeActor([]).actor;
		const services: PlatformServices = {
			sessionFactory: {
				connect: vi.fn(
					() =>
						new Promise<SessionActor>((resolve) => {
							releaseFirstConnect = () => resolve(actor);
						}),
				),
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		const firstConnect = controller.connectServer(server);
		const secondConnect = controller.connectServer(server);
		await Promise.resolve();

		expect(services.sessionFactory.connect).toHaveBeenCalledTimes(1);

		if (releaseFirstConnect == null) {
			throw new Error("expected first connect to be pending");
		}
		releaseFirstConnect();
		await firstConnect;
		await secondConnect;
	});

	it("keeps consuming events while the session actor retries", async () => {
		const { actor } = makeActor([
			{ kind: "Connecting" },
			{
				kind: "Disconnected",
				will_retry: true,
				reason: "relay closed pair before open",
			},
			{ kind: "Connecting" },
			{ kind: "Connected", server_label: "server-a" },
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		await controller.connectServer(server);

		await waitFor(() => {
			expect(workspaceState.getState().connectionState).toBe("connected");
			expect(workspaceState.getState().terminals).toHaveLength(1);
		});
	});

	it("ignores stale connect failures after switching servers", async () => {
		let rejectServerA: ((error: Error) => void) | undefined;
		const serverBActor = makeActor([
			{ kind: "Connecting" },
			{ kind: "Connected", server_label: "server-b" },
		]).actor;
		const services: PlatformServices = {
			sessionFactory: {
				connect: vi.fn((config: ConnectConfig) => {
					if (config.kind === "direct" && config.endpointUrl.endsWith("1001")) {
						return new Promise<SessionActor>((_resolve, reject) => {
							rejectServerA = reject;
						});
					}
					return Promise.resolve(serverBActor);
				}),
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
					endpointUrl: "ws://127.0.0.1:1001",
					resumeTokenHex: null,
					lastConnectedAt: null,
				},
				{
					id: "server-b",
					label: "server-b",
					kind: "direct",
					endpointUrl: "ws://127.0.0.1:1002",
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const [serverA, serverB] = daemonRegistry.getState().daemons;
		expect(serverA).toBeDefined();
		expect(serverB).toBeDefined();
		if (serverA == null || serverB == null) {
			throw new Error("expected seeded daemons");
		}

		void controller.connectServer(serverA).catch((error: unknown) => {
			workspaceState
				.getState()
				.markConnectionFailed(
					error instanceof Error ? error.message : "connection failed",
				);
		});
		await Promise.resolve();
		await controller.connectServer(serverB);

		await waitFor(() => {
			expect(workspaceState.getState().connectionState).toBe("connected");
			expect(workspaceState.getState().activeConnectionServerId).toBe(
				"server-b",
			);
		});

		if (rejectServerA == null) {
			throw new Error("expected server-a connect to be pending");
		}
		rejectServerA(new Error("server-a unavailable"));
		await Promise.resolve();

		expect(workspaceState.getState().connectionState).toBe("connected");
		expect(workspaceState.getState().activeConnectionServerId).toBe("server-b");
	});

	it("clears connection state when the current event stream ends", async () => {
		const { actor } = makeActor(
			[{ kind: "Connecting" }, { kind: "Connected", server_label: "server-a" }],
			{ closeAfterEvents: true },
		);
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		await controller.connectServer(server);

		await waitFor(() => {
			expect(workspaceState.getState().connectionState).toBe("idle");
			expect(workspaceState.getState().activeConnectionServerId).toBeNull();
			expect(controller.getSession()).toBeNull();
		});
	});

	it("cleans up the active session when a runtime error event arrives", async () => {
		const { actor } = makeActor([
			{ kind: "Connecting" },
			{ kind: "Connected", server_label: "server-a" },
			{ kind: "Error", message: "server protocol error" },
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
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		await controller.connectServer(server);

		await waitFor(() => {
			expect(workspaceState.getState().connectionState).toBe("failed");
			expect(workspaceState.getState().activeConnectionServerId).toBeNull();
			expect(controller.getSession()).toBeNull();
		});
	});

	it("disconnects the active server and clears workspace state", async () => {
		const { actor } = makeActor([
			{ kind: "Connecting" },
			{ kind: "Connected", server_label: "server-a" },
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

		const onConnectionReset = vi.fn();
		const controller = new ConnectionController({
			services,
			daemonRegistry,
			uiState,
			workspaceState,
			onInlineError: vi.fn(),
			onConnectionReset,
			onTerminalRemoved: vi.fn(),
			terminalRegistry: makeTerminalRegistry(),
		});

		const server = daemonRegistry.getState().daemons[0];
		expect(server).toBeDefined();
		if (server == null) {
			throw new Error("expected seeded daemon");
		}

		await controller.connectServer(server);
		await new Promise((resolve) => setTimeout(resolve, 0));
		await controller.disconnect();

		expect(actor.close).toHaveBeenCalledTimes(1);
		expect(onConnectionReset).toHaveBeenCalledTimes(2);
		expect(workspaceState.getState().connectionState).toBe("idle");
		expect(workspaceState.getState().activeConnectionServerId).toBeNull();
		expect(workspaceState.getState().terminals).toHaveLength(0);
	});
});
