import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ClientBridge, ConnectConfig } from "@/platform/bridge/types";
import type { AppPlatform } from "@/platform/runtime/platform";
import type { PersistedDaemonRegistry } from "@/state/daemon-registry/daemonRegistry";

vi.mock("@/features/terminals/XTermView", () => ({
	XTermView: ({ title }: { title: string }) => (
		<div data-testid="xterm-view">{title}</div>
	),
}));

const WEB_PLATFORM: AppPlatform = {
	id: "web",
	shell: "desktop",
	bridge: "web",
	embeddedDaemon: false,
};

const EMPTY_REGISTRY = {
	version: 1,
	daemons: [],
	selectedDaemonId: null,
} satisfies PersistedDaemonRegistry;

function cloneRegistry(
	state: PersistedDaemonRegistry | null,
): PersistedDaemonRegistry | null {
	return state == null
		? null
		: (JSON.parse(JSON.stringify(state)) as PersistedDaemonRegistry);
}

function createPairHash(serverId: string, label = "Paired server") {
	return `/#pair=${Buffer.from(
		JSON.stringify({
			v: 1,
			label,
			serverId,
			serverPublicHex: "11".repeat(32),
			relay: {
				url: `wss://relay.example/ws/client?server=${serverId}`,
				pskHex: "22".repeat(32),
			},
		}),
	).toString("base64url")}`;
}

function createDeferred<T>() {
	let resolve: ((value: T) => void) | null = null;
	const promise = new Promise<T>((nextResolve) => {
		resolve = nextResolve;
	});

	return {
		promise,
		resolve(value: T) {
			if (resolve == null) {
				throw new Error("deferred already resolved");
			}
			resolve(value);
			resolve = null;
		},
	};
}

function createEventSource() {
	let queue: unknown[] = [];
	let closed = false;
	let pendingResolve: ((result: IteratorResult<unknown>) => void) | null = null;

	const flush = () => {
		if (pendingResolve == null) {
			return;
		}

		if (queue.length > 0) {
			const value = queue.shift();
			const resolve = pendingResolve;
			pendingResolve = null;
			resolve({ value, done: false });
			return;
		}

		if (closed) {
			const resolve = pendingResolve;
			pendingResolve = null;
			resolve({ value: undefined, done: true });
		}
	};

	return {
		iterable: {
			[Symbol.asyncIterator]() {
				return {
					next: async () => {
						if (queue.length > 0) {
							const value = queue.shift();
							return { value, done: false };
						}
						if (closed) {
							return { value: undefined, done: true };
						}

						return await new Promise<IteratorResult<unknown>>((resolve) => {
							pendingResolve = resolve;
						});
					},
				};
			},
		} as AsyncIterable<unknown>,
		push(event: unknown) {
			queue = [...queue, event];
			flush();
		},
		close() {
			closed = true;
			flush();
		},
	};
}

function createFakeBridge(
	initialRegistry: PersistedDaemonRegistry | null = null,
) {
	const eventSource = createEventSource();

	const bridge = {
		connect: vi.fn(async (_config: ConnectConfig) => {}),
		events: vi.fn(() => eventSource.iterable),
		createTerminal: vi.fn(
			async (_params: {
				cols: number;
				rows: number;
				cwd?: string;
				cmd?: string[];
				shell?: string;
				env?: Record<string, string>;
				scrollbackBytes?: number;
			}) => {},
		),
		sendInput: vi.fn(async (_terminalId: string, _bytes: Uint8Array) => {}),
		resize: vi.fn(
			async (_terminalId: string, _cols: number, _rows: number) => {},
		),
		kill: vi.fn(async (_terminalId: string, _signal: string) => {}),
		exportIdentity: vi.fn(async () => new Uint8Array()),
		importIdentity: vi.fn(async (_blob: Uint8Array) => {}),
		daemonRegistry: {
			load: vi.fn(async () => cloneRegistry(initialRegistry)),
			save: vi.fn(async (_state: PersistedDaemonRegistry) => {}),
		},
		embeddedDaemon: null,
		close: vi.fn(async () => {}),
		pushEvent(event: unknown) {
			eventSource.push(event);
		},
		closeEvents() {
			eventSource.close();
		},
	} satisfies ClientBridge & {
		pushEvent(event: unknown): void;
		closeEvents(): void;
	};

	return bridge;
}

async function loadAppRoot() {
	const module = await import("./AppRoot");
	return module.AppRoot;
}

beforeEach(() => {
	window.localStorage.clear();
	window.history.replaceState(null, "", "/");
	vi.resetModules();
});

afterEach(() => {
	cleanup();
	window.localStorage.clear();
	window.history.replaceState(null, "", "/");
});

describe("AppRoot", () => {
	it("does not consume bridge events before a connection starts", async () => {
		const bridge = createFakeBridge(EMPTY_REGISTRY);
		const AppRoot = await loadAppRoot();

		render(
			<AppRoot platform={WEB_PLATFORM} bridgeFactory={async () => bridge} />,
		);

		await screen.findByRole("button", { name: "Direct connection" });
		expect(bridge.events).not.toHaveBeenCalled();
		expect(bridge.connect).not.toHaveBeenCalled();
	});

	it("imports #pair links before auto-connecting saved servers", async () => {
		const pairServerId = "123e4567-e89b-42d3-a456-426614174000";
		const savedServerId = "223e4567-e89b-42d3-a456-426614174000";
		window.history.replaceState(null, "", createPairHash(pairServerId));

		const bridge = createFakeBridge({
			version: 1,
			daemons: [
				{
					id: savedServerId,
					label: "Saved server",
					kind: "relay",
					serverId: savedServerId,
					serverPublicHex: "33".repeat(32),
					relayUrl: `wss://relay.example/ws/client?server=${savedServerId}`,
					relayPskHex: "44".repeat(32),
					resumeTokenHex: null,
					lastConnectedAt: null,
				},
			],
			selectedDaemonId: savedServerId,
		});
		const AppRoot = await loadAppRoot();

		render(
			<AppRoot platform={WEB_PLATFORM} bridgeFactory={async () => bridge} />,
		);

		await waitFor(() => expect(bridge.connect).toHaveBeenCalledTimes(1));
		expect(bridge.connect).toHaveBeenCalledWith({
			kind: "relay",
			relayUrl: `wss://relay.example/ws/client?server=${pairServerId}`,
			serverId: pairServerId,
			pskHex: "22".repeat(32),
			serverPublicHex: "11".repeat(32),
			resumeTokenHex: undefined,
		});
	});

	it("persists a paired server only after the connection becomes live", async () => {
		const pairServerId = "323e4567-e89b-42d3-a456-426614174000";
		window.history.replaceState(null, "", createPairHash(pairServerId));

		const bridge = createFakeBridge(EMPTY_REGISTRY);
		const AppRoot = await loadAppRoot();

		render(
			<AppRoot platform={WEB_PLATFORM} bridgeFactory={async () => bridge} />,
		);

		await waitFor(() => expect(bridge.connect).toHaveBeenCalledTimes(1));
		expect(bridge.daemonRegistry.save).not.toHaveBeenCalled();

		bridge.pushEvent({
			kind: "Connected",
			server_label: "Paired terminal",
		});

		await waitFor(() =>
			expect(
				bridge.daemonRegistry.save.mock.calls.some(([state]) =>
					state.daemons.some((daemon) => daemon.id === pairServerId),
				),
			).toBe(true),
		);
		expect(window.location.hash).toBe("");
	});

	it("keeps a single bridge event subscription and surfaces disconnect reasons", async () => {
		const serverId = "423e4567-e89b-42d3-a456-426614174000";
		const bridge = createFakeBridge({
			version: 1,
			daemons: [
				{
					id: serverId,
					label: "Saved server",
					kind: "relay",
					serverId,
					serverPublicHex: "33".repeat(32),
					relayUrl: `wss://relay.example/ws/client?server=${serverId}`,
					relayPskHex: "44".repeat(32),
					resumeTokenHex: null,
					lastConnectedAt: null,
				},
			],
			selectedDaemonId: serverId,
		});
		const AppRoot = await loadAppRoot();

		render(
			<AppRoot platform={WEB_PLATFORM} bridgeFactory={async () => bridge} />,
		);

		await waitFor(() => expect(bridge.connect).toHaveBeenCalledTimes(1));
		await waitFor(() => expect(bridge.events).toHaveBeenCalledTimes(1));

		bridge.pushEvent({
			kind: "Disconnected",
			will_retry: false,
			reason: "lost",
		});

		await waitFor(() =>
			expect(screen.getByRole("alert")).toHaveTextContent("lost"),
		);
		expect(bridge.events).toHaveBeenCalledTimes(1);
	});

	it("keeps the live bridge when a stale StrictMode mount resolves late", async () => {
		const firstBridge = createFakeBridge(EMPTY_REGISTRY);
		const secondBridge = createFakeBridge(EMPTY_REGISTRY);
		const firstBridgeDeferred = createDeferred<typeof firstBridge>();
		const secondBridgeDeferred = createDeferred<typeof secondBridge>();
		const bridgeFactory = vi
			.fn<(platform: AppPlatform) => Promise<ClientBridge>>()
			.mockImplementationOnce(async () => firstBridgeDeferred.promise)
			.mockImplementationOnce(async () => secondBridgeDeferred.promise);
		const AppRoot = await loadAppRoot();

		render(
			<StrictMode>
				<AppRoot platform={WEB_PLATFORM} bridgeFactory={bridgeFactory} />
			</StrictMode>,
		);

		await waitFor(() => expect(bridgeFactory).toHaveBeenCalledTimes(2));
		secondBridgeDeferred.resolve(secondBridge);
		await waitFor(() =>
			expect(secondBridge.daemonRegistry.load).toHaveBeenCalledTimes(1),
		);

		firstBridgeDeferred.resolve(firstBridge);
		await Promise.resolve();
		await Promise.resolve();

		expect(firstBridge.close).not.toHaveBeenCalled();
		expect(secondBridge.close).not.toHaveBeenCalled();
	});
});
