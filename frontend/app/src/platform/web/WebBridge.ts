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
	private constructor(private readonly client: CliPocketClient) {}

	readonly daemonRegistry: DaemonRegistryBridge = {
		load: async () => loadDaemonRegistryFromLocalStorage(),
		save: async (state) => {
			saveDaemonRegistryToLocalStorage(state);
		},
	};

	readonly embeddedDaemon = null;

	static async create() {
		await init();
		return new WebBridge(new CliPocketClient());
	}

	async connect(config: ConnectConfig) {
		if (config.kind === "direct") {
			await this.client.connect({
				kind: "direct",
				endpoint_url: config.endpointUrl,
				resume_token_hex: config.resumeTokenHex ?? null,
			});
			return;
		}

		await this.client.connect({
			kind: "relay",
			relay_url: config.relayUrl,
			server_id: config.serverId,
			psk_hex: config.pskHex,
			server_public_hex: config.serverPublicHex,
			resume_token_hex: config.resumeTokenHex ?? null,
		});
	}

	events(): AsyncIterable<unknown> {
		const client = this.client;
		return {
			[Symbol.asyncIterator]: () => ({
				next: async () => {
					const value = await client.next_event();
					return value == null
						? { value: undefined, done: true }
						: { value, done: false };
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
				scrollback_bytes: params.scrollbackBytes ?? null,
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
		this.client.close();
	}
}
