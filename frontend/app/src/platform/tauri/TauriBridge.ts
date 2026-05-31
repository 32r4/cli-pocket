import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PersistedDaemonRegistry } from "@/state/daemon-registry/daemonRegistry";
import type {
	ClientBridge,
	ConnectConfig,
	CreateTerminalParams,
	DaemonRegistryBridge,
	EmbeddedDaemonBridge,
	TerminalInfoRecord,
	TerminalSnapshotRecord,
} from "../bridge/types";

const EVENT_CHANNEL = "cli_pocket:event";

interface TauriBridgeOptions {
	embeddedDaemon: boolean;
}

export class TauriBridge implements ClientBridge {
	readonly daemonRegistry: DaemonRegistryBridge = {
		load: () =>
			invoke<PersistedDaemonRegistry | null>("cli_pocket_load_daemon_registry"),
		save: async (state) => {
			await invoke("cli_pocket_save_daemon_registry", { state });
		},
	};

	readonly embeddedDaemon: EmbeddedDaemonBridge | null;

	constructor({ embeddedDaemon }: TauriBridgeOptions) {
		this.embeddedDaemon = embeddedDaemon
			? {
					localEndpoint: () =>
						invoke<string>("cli_pocket_local_daemon_endpoint"),
					pairUrl: () => invoke<string>("cli_pocket_daemon_pair_url"),
					restart: async () => {
						await invoke("cli_pocket_daemon_restart");
					},
				}
			: null;
	}

	async connect(config: ConnectConfig) {
		await invoke("cli_pocket_connect", { config });
	}

	events(): AsyncIterable<unknown> {
		return {
			[Symbol.asyncIterator]: async function* () {
				const queue: unknown[] = [];
				await listen(EVENT_CHANNEL, (event) => {
					queue.push(event.payload);
				});

				while (true) {
					const next = queue.shift();
					if (next !== undefined) {
						yield next;
					}
					await new Promise((resolve) => setTimeout(resolve, 16));
				}
			},
		};
	}

	async createTerminal(params: CreateTerminalParams) {
		await invoke("cli_pocket_create_terminal", { params });
	}

	async listTerminals() {
		return await invoke<TerminalInfoRecord[]>("cli_pocket_list_terminals");
	}

	async openTerminal(terminalId: string) {
		return await invoke<TerminalSnapshotRecord>("cli_pocket_open_terminal", {
			terminalId,
		});
	}

	async sendInput(terminalId: string, bytes: Uint8Array) {
		await invoke("cli_pocket_send_input", {
			terminalId,
			bytes: Array.from(bytes),
		});
	}

	async resize(terminalId: string, cols: number, rows: number) {
		await invoke("cli_pocket_resize", { terminalId, cols, rows });
	}

	async kill(terminalId: string, signal: string) {
		await invoke("cli_pocket_kill", { terminalId, signal });
	}

	async exportIdentity() {
		const raw = await invoke<Uint8Array | number[] | string>(
			"cli_pocket_export_identity",
		);
		if (raw instanceof Uint8Array) {
			return raw;
		}
		if (Array.isArray(raw)) {
			return new Uint8Array(raw);
		}
		return new TextEncoder().encode(raw);
	}

	async importIdentity(blob: Uint8Array) {
		await invoke("cli_pocket_import_identity", { blob: Array.from(blob) });
	}

	async close() {
		await invoke("cli_pocket_close");
	}
}
