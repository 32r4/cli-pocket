import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PersistedDaemonRegistry } from "@/state/daemon-registry/daemonRegistry";
import { AsyncValueQueue } from "../bridge/asyncValueQueue";
import { findCreatedTerminal } from "../bridge/terminalDiff";
import type {
	ConnectConfig,
	CreateTerminalParams,
	HostAdapter,
	PlatformServices,
	RegistryAdapter,
	SessionActor,
	TerminalInfoRecord,
	TerminalSnapshotRecord,
} from "../bridge/types";

interface TauriBridgeOptions {
	embeddedDaemon: boolean;
}

function terminalListEvent(terminals: TerminalInfoRecord[]) {
	return {
		kind: "TerminalList",
		terminals,
	};
}

class TauriEventSource {
	private readonly queue = new AsyncValueQueue<unknown>();
	private readonly unlistenPromise: Promise<() => void>;
	private closed = false;

	constructor(eventChannel: string) {
		this.unlistenPromise = listen(eventChannel, (event) => {
			this.queue.push(event.payload);
		});
	}

	push(value: unknown) {
		this.queue.push(value);
	}

	async next() {
		return await this.queue.next();
	}

	async close() {
		if (this.closed) {
			return;
		}

		this.closed = true;
		this.queue.close();
		const unlisten = await this.unlistenPromise;
		unlisten();
	}
}

class TauriSessionActor implements SessionActor {
	constructor(private readonly eventSource: TauriEventSource) {}

	events(): AsyncIterable<unknown> {
		return {
			[Symbol.asyncIterator]: () => ({
				next: async () => await this.eventSource.next(),
			}),
		};
	}

	async refreshTerminals() {
		const terminals = await this.loadTerminals();
		this.eventSource.push(terminalListEvent(terminals));
	}

	async openTerminal(terminalId: string) {
		return await invoke<TerminalSnapshotRecord>("cli_pocket_open_terminal", {
			terminalId,
		});
	}

	async createTerminal(params: CreateTerminalParams) {
		const before = await this.loadTerminals();
		await invoke("cli_pocket_create_terminal", { params });
		const after = await this.loadTerminals();
		this.eventSource.push(terminalListEvent(after));
		return findCreatedTerminal(before, after);
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

	async close() {
		await this.eventSource.close();
		await invoke("cli_pocket_close");
	}

	private async loadTerminals() {
		return await invoke<TerminalInfoRecord[]>("cli_pocket_list_terminals");
	}
}

export class TauriBridge implements PlatformServices {
	readonly sessionFactory = {
		connect: async (config: ConnectConfig) => {
			const eventChannel = `cli_pocket:event:${crypto.randomUUID()}`;
			const events = new TauriEventSource(eventChannel);
			try {
				await invoke("cli_pocket_connect", { config, eventChannel });
				return new TauriSessionActor(events);
			} catch (error: unknown) {
				await events.close();
				throw error;
			}
		},
	};

	readonly registry: RegistryAdapter = {
		load: () =>
			invoke<PersistedDaemonRegistry | null>("cli_pocket_load_daemon_registry"),
		save: async (state) => {
			await invoke("cli_pocket_save_daemon_registry", { state });
		},
		exportIdentity: async () => await this.exportIdentity(),
		importIdentity: async (blob) => await this.importIdentity(blob),
	};

	readonly host: HostAdapter | null;

	constructor({ embeddedDaemon }: TauriBridgeOptions) {
		this.host = embeddedDaemon
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

	private async exportIdentity() {
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

	private async importIdentity(blob: Uint8Array) {
		await invoke("cli_pocket_import_identity", { blob: Array.from(blob) });
	}
}
