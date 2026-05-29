import { z } from "zod";
import type { DaemonRecord } from "@/state/daemon-registry/types";

const PairingOfferPayload = z.object({
	v: z.literal(1),
	label: z.string().trim().min(1).optional(),
	serverId: z.string().uuid(),
	serverPublicHex: z.string().regex(/^[0-9a-f]{64}$/i),
	relay: z.object({
		url: z.string().url(),
		pskHex: z.string().regex(/^[0-9a-f]{64}$/i),
	}),
});

function decodeBase64Url(value: string) {
	try {
		const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
		const padding = normalized.length % 4;
		const padded =
			padding === 0 ? normalized : normalized + "=".repeat(4 - padding);
		const binary = atob(padded);
		const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));

		return new TextDecoder().decode(bytes);
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
		id: payload.serverId,
		label: payload.label ?? payload.serverId,
		kind: "relay",
		serverId: payload.serverId,
		serverPublicHex: payload.serverPublicHex,
		relayUrl: payload.relay.url,
		relayPskHex: payload.relay.pskHex,
		resumeTokenHex: null,
		lastConnectedAt: null,
	};
}
