export type ConnectConfig =
	| {
			kind: "direct";
			endpointUrl: string;
			resumeTokenHex?: string;
	  }
	| {
			kind: "relay";
			relayUrl: string;
			hostId: string;
			pskHex: string;
			serverPublicHex: string;
			resumeTokenHex?: string;
	  };

export interface TerminalSummary {
	id: string;
	title: string;
	status: "connecting" | "ready" | "closed";
}

export interface CreateTerminalParams {
	cols: number;
	rows: number;
	cwd?: string;
	cmd?: string[];
	shell?: string;
	env?: Record<string, string>;
	scrollbackBytes?: number;
}

export interface ClientBridge {
	connect(config: ConnectConfig): Promise<void>;
	events(): AsyncIterable<unknown>;
	createTerminal(params: CreateTerminalParams): Promise<void>;
	sendInput(terminalId: string, bytes: Uint8Array): Promise<void>;
	resize(terminalId: string, cols: number, rows: number): Promise<void>;
	kill(terminalId: string, signal: string): Promise<void>;
	exportIdentity(): Promise<Uint8Array>;
	importIdentity(blob: Uint8Array): Promise<void>;
	close(): Promise<void>;
}
