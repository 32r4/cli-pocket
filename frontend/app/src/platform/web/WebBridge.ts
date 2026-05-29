import init, {
	close_client,
	connect_client,
	create_client_terminal,
	export_client_identity,
	import_client_identity,
	kill_client_terminal,
	next_client_event,
	resize_client_terminal,
	send_client_input,
} from "cli-pocket-client-core-wasm";
import type {
	ClientBridge,
	ConnectConfig,
	CreateTerminalParams,
} from "../bridge/types";

export class WebBridge implements ClientBridge {
	static async create() {
		await init();
		return new WebBridge();
	}

	async connect(config: ConnectConfig) {
		if (config.kind === "direct") {
			await connect_client({
				kind: "direct",
				endpoint_url: config.endpointUrl,
				resume_token_hex: config.resumeTokenHex ?? null,
			});
			return;
		}

		await connect_client({
			kind: "relay",
			relay_url: config.relayUrl,
			server_id: config.serverId,
			psk_hex: config.pskHex,
			server_public_hex: config.serverPublicHex,
			resume_token_hex: config.resumeTokenHex ?? null,
		});
	}

	events(): AsyncIterable<unknown> {
		return {
			[Symbol.asyncIterator]: () => ({
				next: async () => {
					const value = await next_client_event();
					return value == null
						? { value: undefined, done: true }
						: { value, done: false };
				},
			}),
		};
	}

	async createTerminal(params: CreateTerminalParams) {
		await create_client_terminal(
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
		await send_client_input(bytes);
	}

	async resize(_terminalId: string, cols: number, rows: number) {
		await resize_client_terminal(cols, rows);
	}

	async kill(_terminalId: string, _signal: string) {
		await kill_client_terminal();
	}

	async exportIdentity(): Promise<Uint8Array> {
		return new TextEncoder().encode(export_client_identity());
	}

	async importIdentity(blob: Uint8Array) {
		await import_client_identity(new TextDecoder().decode(blob));
	}

	async localDaemonEndpoint(): Promise<string> {
		throw new Error("local daemon is only available in desktop");
	}

	async daemonPairUrl(): Promise<string> {
		throw new Error("embedded daemon is only available in desktop");
	}

	async daemonRestart() {
		throw new Error("embedded daemon is only available in desktop");
	}

	async close() {
		await close_client();
	}
}
