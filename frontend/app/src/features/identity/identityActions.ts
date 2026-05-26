import type { ClientBridge } from "@/platform/bridge/types";

export async function exportIdentity(client: ClientBridge) {
	const bytes = await client.exportIdentity();
	const copy = new Uint8Array(bytes.byteLength);
	copy.set(bytes);
	const blob = new Blob([copy.buffer], { type: "application/octet-stream" });
	return URL.createObjectURL(blob);
}

export async function importIdentity(client: ClientBridge, file: File) {
	const bytes = new Uint8Array(await file.arrayBuffer());
	await client.importIdentity(bytes);
}
