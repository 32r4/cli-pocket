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
	cols: number;
	rows: number;
	createdAtUnixMs: number;
	attachedClients: number;
}

export interface TerminalInfoRecord {
	terminal: string;
	cols: number;
	rows: number;
	created_at_unix_ms: number;
	label: string | null;
	attached_clients: number;
}

export interface TerminalOpenAckRecord {
	stream_id: number;
	info: TerminalInfoRecord;
	start_seq: number;
	end_seq: number;
	render_bytes_b64: string;
	has_more_history: boolean;
}

export interface TerminalHistoryPageRecord {
	terminal_id: string;
	start_seq: number;
	end_seq: number;
	bytes_b64: string;
	has_more: boolean;
}

export interface ServerConfigRecord {
	scrollback_bytes: number;
}

export interface PairingQrCodeRecord {
	url: string;
	svg: string;
	terminal: string;
}

export interface CreateTerminalParams {
	cols: number;
	rows: number;
	cwd?: string;
	cmd?: string[];
	shell?: string;
	env?: Record<string, string>;
}

export interface SessionActor {
	events(): AsyncIterable<unknown>;
	refreshTerminals(): Promise<void>;
	openTerminal(terminalId: string): Promise<TerminalOpenAckRecord>;
	readHistory(
		terminalId: string,
		before: number | null,
		maxBytes: number,
	): Promise<TerminalHistoryPageRecord>;
	createTerminal(
		params: CreateTerminalParams,
	): Promise<TerminalInfoRecord | null>;
	getServerConfig(): Promise<ServerConfigRecord>;
	setServerConfig(config: ServerConfigRecord): Promise<ServerConfigRecord>;
	sendInput(terminalId: string, bytes: Uint8Array): Promise<void>;
	resize(terminalId: string, cols: number, rows: number): Promise<void>;
	kill(terminalId: string, signal: string): Promise<void>;
	close(): Promise<void>;
}

export interface SessionFactory {
	connect(config: ConnectConfig): Promise<SessionActor>;
}

export interface IdentityAdapter {
	exportIdentity(): Promise<Uint8Array>;
	importIdentity(blob: Uint8Array): Promise<void>;
}

export interface RegistryAdapter extends IdentityAdapter {
	load(): Promise<PersistedDaemonRegistry | null>;
	save(state: PersistedDaemonRegistry): Promise<void>;
}

export interface HostAdapter {
	localEndpoint(): Promise<string>;
	pairUrl(): Promise<string>;
	pairQrCode(): Promise<PairingQrCodeRecord>;
	restart(): Promise<void>;
}

export interface PlatformServices {
	sessionFactory: SessionFactory;
	registry: RegistryAdapter;
	host: HostAdapter | null;
}
