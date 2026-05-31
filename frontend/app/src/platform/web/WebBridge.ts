import init, { CliPocketClient } from "cli-pocket-client-core-wasm";
import {
	type PersistedDaemonRegistry,
	parsePersistedDaemonRegistry,
} from "@/state/daemon-registry/daemonRegistry";
import type {
	ClientBridge,
	ConnectConfig,
	CreateTerminalParams,
	DaemonRegistryBridge,
} from "../bridge/types";

const STORAGE_KEY = "cli-pocket/daemon-registry/v1";
let wasmInitPromise: Promise<void> | null = null;

function ensureWasmInitialized() {
	if (wasmInitPromise != null) {
		return wasmInitPromise;
	}

	wasmInitPromise = init().then(
		() => undefined,
		(error) => {
			wasmInitPromise = null;
			throw error;
		},
	);

	return wasmInitPromise;
}

function loadDaemonRegistryFromLocalStorage(): PersistedDaemonRegistry | null {
	if (typeof window === "undefined") {
		return null;
	}

	try {
		const raw = window.localStorage.getItem(STORAGE_KEY);
		if (raw == null) {
			return null;
		}

		return parsePersistedDaemonRegistry(JSON.parse(raw));
	} catch {
		return null;
	}
}

function saveDaemonRegistryToLocalStorage(state: PersistedDaemonRegistry) {
	if (typeof window === "undefined") {
		return;
	}

	try {
		window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
	} catch {}
}

export class WebBridge implements ClientBridge {
	private readonly eventQueue: unknown[] = [];
	private eventWaiters: Array<{
		resolve: (result: IteratorResult<unknown>) => void;
		reject: (error: unknown) => void;
	}> = [];
	private eventPumpStarted = false;
	private eventStreamClosed = false;

	private constructor(private readonly client: CliPocketClient) {}

	readonly daemonRegistry: DaemonRegistryBridge = {
		load: async () => loadDaemonRegistryFromLocalStorage(),
		save: async (state) => {
			saveDaemonRegistryToLocalStorage(state);
		},
	};

	readonly embeddedDaemon = null;

	static async create() {
		await ensureWasmInitialized();
		return new WebBridge(new CliPocketClient());
	}

	async connect(config: ConnectConfig) {
		// Reset event pump state before connecting, as connect() creates a new event receiver
		this.eventStreamClosed = false;
		this.eventPumpStarted = false;
		this.eventQueue = [];
		this.eventWaiters = [];

		if (config.kind === "direct") {
			await this.client.connect(
				JSON.stringify({
					kind: "direct",
					endpointUrl: config.endpointUrl,
					resumeTokenHex: config.resumeTokenHex ?? null,
				}),
			);
			return;
		}

		await this.client.connect(
			JSON.stringify({
				kind: "relay",
				relayUrl: config.relayUrl,
				serverId: config.serverId,
				pskHex: config.pskHex,
				serverPublicHex: config.serverPublicHex,
				resumeTokenHex: config.resumeTokenHex ?? null,
			}),
		);
	}

	events(): AsyncIterable<unknown> {
		this.startEventPump();

		return {
			[Symbol.asyncIterator]: () => ({
				next: async () => {
					const queued = this.eventQueue.shift();
					if (queued !== undefined) {
						return { value: queued, done: false };
					}
					if (this.eventStreamClosed) {
						return { value: undefined, done: true };
					}

					return await this.waitForNextEvent();
				},
			}),
		};
	}

	async createTerminal(params: CreateTerminalParams) {
		await this.client.create_terminal(
			JSON.stringify({
				cols: params.cols,
				rows: params.rows,
				cwd: params.cwd ?? null,
				cmd: params.cmd ?? [],
				env: Object.entries(params.env ?? {}),
				scrollbackBytes: params.scrollbackBytes ?? null,
			}),
		);
	}

	async sendInput(_terminalId: string, bytes: Uint8Array) {
		await this.client.send_input(bytes);
	}

	async resize(_terminalId: string, cols: number, rows: number) {
		await this.client.resize(cols, rows);
	}

	async kill(_terminalId: string, _signal: string) {
		await this.client.kill();
	}

	async exportIdentity(): Promise<Uint8Array> {
		return new TextEncoder().encode(this.client.export_identity());
	}

	async importIdentity(blob: Uint8Array) {
		await this.client.import_identity(new TextDecoder().decode(blob));
	}

	async close() {
		this.eventStreamClosed = true;
		for (const waiter of this.eventWaiters) {
			waiter.resolve({ value: undefined, done: true });
		}
		this.eventWaiters = [];
		this.client.close();
	}

	private waitForNextEvent() {
		return new Promise<IteratorResult<unknown>>((resolve, reject) => {
			this.eventWaiters = [...this.eventWaiters, { resolve, reject }];
		});
	}

	private startEventPump() {
		if (this.eventPumpStarted) {
			return;
		}

		this.eventPumpStarted = true;
		void this.pumpEvents();
	}

	private async pumpEvents() {
		try {
			while (!this.eventStreamClosed) {
				const value = await this.client.next_event();
				if (value == null) {
					this.eventStreamClosed = true;
					break;
				}
				this.pushEvent(value);
			}
		} catch (error) {
			this.eventStreamClosed = true;
			this.rejectWaiters(error);
			return;
		}

		for (const waiter of this.eventWaiters) {
			waiter.resolve({ value: undefined, done: true });
		}
		this.eventWaiters = [];
	}

	private pushEvent(value: unknown) {
		const waiter = this.eventWaiters.shift();
		if (waiter != null) {
			waiter.resolve({ value, done: false });
			return;
		}

		this.eventQueue.push(value);
	}

	private rejectWaiters(error: unknown) {
		const reason =
			error instanceof Error ? error : new Error("event stream failed");
		for (const waiter of this.eventWaiters) {
			waiter.reject(reason);
		}
		this.eventWaiters = [];
	}
}
