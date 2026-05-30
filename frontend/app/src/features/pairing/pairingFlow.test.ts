import { describe, expect, it, vi } from "vitest";
import { pairAndStoreDaemon } from "./pairingFlow";

describe("pairAndStoreDaemon", () => {
	it("connects and stores a daemon imported from a relay pairing offer url", async () => {
		const connectDaemon = vi.fn().mockResolvedValue(undefined);
		const upsertDaemon = vi.fn();
		const serverId = "123e4567-e89b-12d3-a456-426614174000";
		const pairUrl = `https://cli-pocket.32r4.asia/#pair=${Buffer.from(
			JSON.stringify({
				v: 1,
				label: "Primary Server",
				serverId,
				serverPublicHex: "11".repeat(32),
				relay: {
					url: "wss://relay.example/ws/client?server=123",
					pskHex: "22".repeat(32),
				},
			}),
		).toString("base64url")}`;

		await pairAndStoreDaemon(pairUrl, connectDaemon, upsertDaemon);

		expect(connectDaemon).toHaveBeenCalledTimes(1);
		expect(connectDaemon).toHaveBeenCalledWith(serverId, {
			kind: "relay",
			relayUrl: "wss://relay.example/ws/client?server=123",
			serverId,
			pskHex: "22".repeat(32),
			serverPublicHex: "11".repeat(32),
			resumeTokenHex: undefined,
		});
		expect(upsertDaemon).toHaveBeenCalledTimes(1);
		expect(upsertDaemon).toHaveBeenCalledWith({
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

	it("does not store the daemon when pairing fails", async () => {
		const connectDaemon = vi
			.fn()
			.mockRejectedValue(new Error("pairing failed"));
		const upsertDaemon = vi.fn();
		const pairUrl = `https://cli-pocket.32r4.asia/#pair=${Buffer.from(
			JSON.stringify({
				v: 1,
				serverId: "123e4567-e89b-12d3-a456-426614174000",
				serverPublicHex: "11".repeat(32),
				relay: {
					url: "wss://relay.example/ws/client?server=123",
					pskHex: "22".repeat(32),
				},
			}),
		).toString("base64url")}`;

		await expect(
			pairAndStoreDaemon(pairUrl, connectDaemon, upsertDaemon),
		).rejects.toThrow("pairing failed");
		expect(upsertDaemon).not.toHaveBeenCalled();
	});
});
