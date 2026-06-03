declare const __APP_PLATFORM__: import("@/platform/runtime/platform").AppPlatformId;

declare module "cli-pocket-client-core-wasm" {
	export default function init(): Promise<unknown>;

	export class CliPocketClient {
		constructor();
		connect(config: string): Promise<void>;
		create_terminal(paramsJson: string): Promise<void>;
		get_server_config(): Promise<unknown>;
		list_terminals(): Promise<unknown>;
		activate_terminal(terminalId: string): Promise<unknown>;
		read_history(
			terminalId: string,
			before: number | null,
			maxBytes: number,
		): Promise<unknown>;
		set_server_config(configJson: string): Promise<unknown>;
		send_input(terminalId: string, data: Uint8Array): Promise<void>;
		resize(terminalId: string, cols: number, rows: number): Promise<void>;
		kill(terminalId: string): Promise<void>;
		next_event(): Promise<unknown>;
		export_identity(): string;
		import_identity(blob: string): Promise<void>;
		close(): void;
	}
}
