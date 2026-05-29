import type { PersistedDaemonRegistry } from "@/state/daemon-registry/daemonRegistry";

export type ConnectConfig =
	| {
			kind: "direct";
			endpointUrl: string;
			resumeTokenHex?: string;
	  }
	| {
			kind: "relay";
			relayUrl: string;
			serverId: string;
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

export interface DaemonRegistryBridge {
	load(): Promise<PersistedDaemonRegistry | null>;
	save(state: PersistedDaemonRegistry): Promise<void>;
}

export interface EmbeddedDaemonBridge {
	localEndpoint(): Promise<string>;
	pairUrl(): Promise<string>;
	restart(): Promise<void>;
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
	daemonRegistry: DaemonRegistryBridge;
	embeddedDaemon: EmbeddedDaemonBridge | null;
	close(): Promise<void>;
}
