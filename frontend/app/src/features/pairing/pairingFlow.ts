import type { ConnectConfig } from "@/platform/bridge/types";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { importPairingOfferUrl } from "./pairingOffer";

function toConnectConfig(daemon: DaemonRecord): ConnectConfig {
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

export async function pairAndStoreDaemon(
	rawUrl: string,
	connectDaemon: (serverId: string, config: ConnectConfig) => Promise<void>,
	upsertDaemon: (daemon: DaemonRecord) => void,
) {
	const daemon = importPairingOfferUrl(rawUrl);
	await connectDaemon(daemon.id, toConnectConfig(daemon));
	upsertDaemon(daemon);
	return daemon;
}
