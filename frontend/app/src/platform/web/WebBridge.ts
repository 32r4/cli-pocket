import init, { CliPocketClient } from "cli-pocket-client-core-wasm";
import {
	type PersistedDaemonRegistry,
	parsePersistedDaemonRegistry,
} from "@/state/daemon-registry/daemonRegistry";
import { AsyncValueQueue } from "../bridge/asyncValueQueue";
import type {
	ConnectConfig,
	CreateTerminalParams,
	PlatformServices,
	RegistryAdapter,
	ServerConfigRecord,
	SessionActor,
	TerminalHistoryPageRecord,
	TerminalInfoRecord,
	TerminalOpenAckRecord,
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

function terminalListEvent(terminals: TerminalInfoRecord[]) {
	return {
		kind: "TerminalList",
		terminals,
	};
}

class WebSessionActor implements SessionActor {
	private readonly queue = new AsyncValueQueue<unknown>();
	private closed = false;
	private pumpStarted = false;

	constructor(private readonly client: CliPocketClient) {}

	events(): AsyncIterable<unknown> {
		if (!this.pumpStarted) {
			this.pumpStarted = true;
			void this.pumpEvents();
		}

		return {
			[Symbol.asyncIterator]: () => ({
				next: async () => await this.queue.next(),
			}),
		};
	}

	async refreshTerminals() {
		const terminals = await this.loadTerminals();
		this.queue.push(terminalListEvent(terminals));
	}

	async openTerminal(terminalId: string): Promise<TerminalOpenAckRecord> {
		return (await this.client.open_terminal(
			terminalId,
		)) as TerminalOpenAckRecord;
	}

	async readHistory(
		terminalId: string,
		before: number | null,
		maxBytes: number,
	): Promise<TerminalHistoryPageRecord> {
		return (await this.client.read_history(
			terminalId,
			before,
			maxBytes,
		)) as TerminalHistoryPageRecord;
	}

	async createTerminal(params: CreateTerminalParams) {
		await this.client.create_terminal(
			JSON.stringify({
				cols: params.cols,
				rows: params.rows,
				cwd: params.cwd ?? null,
				cmd: params.cmd ?? [],
				env: Object.entries(params.env ?? {}),
			}),
		);
		return null;
	}

	async getServerConfig(): Promise<ServerConfigRecord> {
		return (await this.client.get_server_config()) as ServerConfigRecord;
	}

	async setServerConfig(
		config: ServerConfigRecord,
	): Promise<ServerConfigRecord> {
		return (await this.client.set_server_config(
			JSON.stringify(config),
		)) as ServerConfigRecord;
	}

	async sendInput(terminalId: string, bytes: Uint8Array) {
		await this.client.send_input(terminalId, bytes);
	}

	async resize(terminalId: string, cols: number, rows: number) {
		await this.client.resize(terminalId, cols, rows);
	}

	async kill(terminalId: string, _signal: string) {
		await this.client.kill(terminalId);
	}

	async close() {
		this.closed = true;
		this.queue.close();
		this.client.close();
	}

	private async loadTerminals() {
		return (await this.client.list_terminals()) as TerminalInfoRecord[];
	}

	private async pumpEvents() {
		try {
			while (!this.closed) {
				const event = await this.client.next_event();
				if (event == null) {
					this.queue.close();
					return;
				}
				this.queue.push(event);
			}
		} catch (error: unknown) {
			if (this.closed) {
				return;
			}
			this.queue.fail(error);
		}
	}
}

export class WebBridge implements PlatformServices {
	readonly sessionFactory = {
		connect: async (config: ConnectConfig) => {
			const client = new CliPocketClient();
			if (config.kind === "direct") {
				await client.connect(
					JSON.stringify({
						kind: "direct",
						endpointUrl: config.endpointUrl,
						resumeTokenHex: config.resumeTokenHex ?? null,
					}),
				);
			} else {
				await client.connect(
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

			return new WebSessionActor(client);
		},
	};

	readonly registry: RegistryAdapter = {
		load: async () => loadDaemonRegistryFromLocalStorage(),
		save: async (state) => {
			saveDaemonRegistryToLocalStorage(state);
		},
		exportIdentity: async () => await this.exportIdentity(),
		importIdentity: async (blob) => await this.importIdentity(blob),
	};

	readonly host = null;

	static async create() {
		await ensureWasmInitialized();
		return new WebBridge();
	}

	private async exportIdentity(): Promise<Uint8Array> {
		const client = new CliPocketClient();
		try {
			return new TextEncoder().encode(client.export_identity());
		} finally {
			client.close();
		}
	}

	private async importIdentity(blob: Uint8Array) {
		const client = new CliPocketClient();
		try {
			await client.import_identity(new TextDecoder().decode(blob));
		} finally {
			client.close();
		}
	}
}
