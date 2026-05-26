export interface DaemonRecord {
	id: string;
	label: string;
	endpointUrl: string;
	serverPublicHex: string;
	resumeTokenHex: string | null;
	lastConnectedAt: number | null;
}
