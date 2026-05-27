import type { DaemonRecord } from "@/state/daemon-registry/types";
import { importPairingOfferUrl } from "./pairingOffer";

export async function pairAndStoreDaemon(
	rawUrl: string,
	upsertDaemon: (daemon: DaemonRecord) => void,
) {
	upsertDaemon(importPairingOfferUrl(rawUrl));
}
