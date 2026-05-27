import { z } from "zod";
import type { DaemonRecord } from "@/state/daemon-registry/types";

const PairingOfferPayload = z.object({
	v: z.literal(1),
	label: z.string().trim().min(1).optional(),
	hostId: z.string().uuid(),
	serverPublicHex: z.string().regex(/^[0-9a-f]{64}$/i),
	relay: z.object({
		url: z.string().url(),
		pskHex: z.string().regex(/^[0-9a-f]{64}$/i),
	}),
});

function decodeBase64Url(value: string) {
	try {
		return Buffer.from(value, "base64url").toString("utf8");
	} catch {
		throw new Error("invalid #pair= payload");
	}
}

export function importPairingOfferUrl(rawUrl: string): DaemonRecord {
	const url = new URL(rawUrl);
	const fragment = url.hash.startsWith("#") ? url.hash.slice(1) : url.hash;
	const pairValue = new URLSearchParams(fragment).get("pair");
	if (!pairValue) {
		throw new Error("pairing url must include #pair=");
	}

	const decoded = decodeBase64Url(pairValue);
	let parsedJson: unknown;
	try {
		parsedJson = JSON.parse(decoded);
	} catch {
		throw new Error("invalid #pair= payload");
	}

	const payload = PairingOfferPayload.parse(parsedJson);

	return {
		id: payload.hostId,
		label: payload.label ?? payload.hostId,
		kind: "relay",
		hostId: payload.hostId,
		serverPublicHex: payload.serverPublicHex,
		relayUrl: payload.relay.url,
		relayPskHex: payload.relay.pskHex,
		resumeTokenHex: null,
		lastConnectedAt: null,
	};
}
