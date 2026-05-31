import type { IdentityAdapter } from "@/platform/bridge/types";

export async function exportIdentity(identity: IdentityAdapter) {
	const bytes = await identity.exportIdentity();
	const copy = new Uint8Array(bytes.byteLength);
	copy.set(bytes);
	const blob = new Blob([copy.buffer], { type: "application/octet-stream" });
	return URL.createObjectURL(blob);
}

export async function importIdentity(identity: IdentityAdapter, file: File) {
	const bytes = new Uint8Array(await file.arrayBuffer());
	await identity.importIdentity(bytes);
}
