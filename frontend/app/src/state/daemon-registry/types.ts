import type { ConnectConfig } from "@/platform/bridge/types";

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

export function daemonRecordToConnectConfig(
	daemon: DaemonRecord,
): ConnectConfig {
	if (daemon.kind === "direct") {
		return {
			kind: "direct",
			endpointUrl: daemon.endpointUrl,
			resumeTokenHex: daemon.resumeTokenHex ?? undefined,
		};
	}

	return {
		kind: "relay",
		relayUrl: daemon.relayUrl,
		serverId: daemon.serverId,
		pskHex: daemon.relayPskHex,
		serverPublicHex: daemon.serverPublicHex,
		resumeTokenHex: daemon.resumeTokenHex ?? undefined,
	};
}
