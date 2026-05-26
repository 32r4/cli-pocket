import { describe, expect, it, vi } from "vitest";
import { pairAndStoreDaemon } from "./pairingFlow";

describe("pairAndStoreDaemon", () => {
	it("stores a daemon returned by the pairing call", async () => {
		const pair = vi.fn().mockResolvedValue({
			server_public_hex: "server-hex",
			client_public_hex: "client-hex",
		});
		const upsertDaemon = vi.fn();

		await pairAndStoreDaemon(
			{
				daemonUrl: "ws://127.0.0.1:7842",
				code: "123456",
			},
			pair,
			upsertDaemon,
		);

		expect(upsertDaemon).toHaveBeenCalledTimes(1);
	});
});
