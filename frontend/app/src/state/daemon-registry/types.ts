interface DaemonRecordBase {
	id: string;
	label: string;
	resumeTokenHex: string | null;
	lastConnectedAt: number | null;
}

export type DaemonRecord =
	| (DaemonRecordBase & {
			kind: "direct";
			endpointUrl: string;
	  })
	| (DaemonRecordBase & {
			kind: "relay";
			serverPublicHex: string;
			serverId: string;
			relayUrl: string;
			relayPskHex: string;
	  });
