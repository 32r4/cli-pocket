import { describe, expect, it } from "vitest";
import { importPairingOfferUrl } from "./pairingOffer";

function encodePairingPayload(payload: unknown) {
	return Buffer.from(JSON.stringify(payload)).toString("base64url");
}

describe("importPairingOfferUrl", () => {
	it("imports one relay pairing offer url", () => {
		const serverId = "123e4567-e89b-12d3-a456-426614174000";
		const rawUrl = `https://cli-pocket.32r4.asia/#pair=${encodePairingPayload({
			v: 1,
			label: "Primary Server",
			serverId,
			serverPublicHex: "11".repeat(32),
			relay: {
				url: "wss://relay.example/ws/client?server=123",
				pskHex: "22".repeat(32),
			},
		})}`;

		expect(importPairingOfferUrl(rawUrl)).toEqual({
			id: serverId,
			label: "Primary Server",
			kind: "relay",
			serverId,
			serverPublicHex: "11".repeat(32),
			relayUrl: "wss://relay.example/ws/client?server=123",
			relayPskHex: "22".repeat(32),
			resumeTokenHex: null,
			lastConnectedAt: null,
		});
	});

	it("falls back to server id when label is absent", () => {
		const serverId = "123e4567-e89b-12d3-a456-426614174000";
		const rawUrl = `https://cli-pocket.32r4.asia/#pair=${encodePairingPayload({
			v: 1,
			serverId,
			serverPublicHex: "11".repeat(32),
			relay: {
				url: "wss://relay.example/ws/client?server=123",
				pskHex: "22".repeat(32),
			},
		})}`;

		expect(importPairingOfferUrl(rawUrl).label).toBe(serverId);
	});

	it("rejects malformed fragments", () => {
		expect(() =>
			importPairingOfferUrl("https://cli-pocket.32r4.asia/#pair=not-base64"),
		).toThrow(/pair/i);
		expect(() =>
			importPairingOfferUrl("https://cli-pocket.32r4.asia/"),
		).toThrow(/#pair=/i);
	});
});
