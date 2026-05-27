interface DaemonRecordBase {
	id: string;
	label: string;
	serverPublicHex: string;
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
			hostId: string;
			relayUrl: string;
			relayPskHex: string;
	  });
