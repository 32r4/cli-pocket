import type { DeltaSlice } from "@/types/frame";

export interface TerminalWriter {
  write(data: Uint8Array, callback?: () => void): void;
}

export async function applyDelta(
  term: TerminalWriter,
  delta: DeltaSlice,
): Promise<void> {
  await writeBytes(term, base64ToBytes(delta.bytes_b64));
}

export function base64ToBytes(encoded: string): Uint8Array {
  const binary = atob(encoded);
  const bytes = new Uint8Array(binary.length);

  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }

  return bytes;
}

export async function writeBytes(
  term: TerminalWriter,
  bytes: Uint8Array,
): Promise<void> {
  await new Promise<void>((resolve) => {
    term.write(bytes, resolve);
  });
}
