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
	status: "idle" | "connecting" | "ready" | "error";
	cols: number;
	rows: number;
	createdAtUnixMs: number;
	attachedClients: number;
	error: string | null;
}

export interface TerminalInfoRecord {
	terminal: string;
	cols: number;
	rows: number;
	created_at_unix_ms: number;
	label: string | null;
	attached_clients: number;
}

export interface TerminalSnapshotRecord {
	info: TerminalInfoRecord;
	snapshot_bytes_b64: string;
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
	listTerminals(): Promise<TerminalInfoRecord[]>;
	openTerminal(terminalId: string): Promise<TerminalSnapshotRecord>;
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
