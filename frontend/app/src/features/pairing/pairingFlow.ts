import { z } from "zod";

const PairingInput = z.object({
	daemonUrl: z.string().url(),
	code: z.string().regex(/^\d{6}$/),
});

export async function pairAndStoreDaemon(
	input: { daemonUrl: string; code: string },
	pair: (
		pairingUrl: string,
		code: string,
	) => Promise<{ server_public_hex: string; client_public_hex: string }>,
	upsertDaemon: (daemon: {
		id: string;
		label: string;
		endpointUrl: string;
		serverPublicHex: string;
		resumeTokenHex: string | null;
		lastConnectedAt: number | null;
	}) => void,
) {
	const parsed = PairingInput.parse(input);
	const result = await pair(parsed.daemonUrl, parsed.code);

	upsertDaemon({
		id: result.server_public_hex,
		label: parsed.daemonUrl,
		endpointUrl: parsed.daemonUrl,
		serverPublicHex: result.server_public_hex,
		resumeTokenHex: null,
		lastConnectedAt: null,
	});
}
