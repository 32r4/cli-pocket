import init, { CliPocketClient } from "cli-pocket-client-core-wasm";
import type {
	ClientBridge,
	ConnectConfig,
	CreateTerminalParams,
} from "../bridge/types";

export class WebBridge implements ClientBridge {
	static async create() {
		await init();
		return new WebBridge(new CliPocketClient());
	}

	constructor(private readonly client: CliPocketClient) {}

	async connect(config: ConnectConfig) {
		await this.client.connect({
			endpoint_url: config.endpointUrl,
			server_public_hex: config.serverPublicHex,
			resume_token_hex: config.resumeTokenHex ?? null,
		});
	}

	events(): AsyncIterable<unknown> {
		return {
			[Symbol.asyncIterator]: () => ({
				next: async () => {
					const value = await this.client.next_event();
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
