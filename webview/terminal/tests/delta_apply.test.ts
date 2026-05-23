import { describe, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { applyDelta } from "@/render/delta";
import type { DeltaSlice } from "@/types/frame";

interface FakeTerminal {
  write: Mock<[bytes: Uint8Array, callback?: (() => void) | undefined], void>;
  written: Uint8Array[];
}

function bytesToBase64(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes));
}

function createTerminal(): FakeTerminal {
  const written: Uint8Array[] = [];
  return {
    write: vi.fn((bytes: Uint8Array, callback?: () => void) => {
      written.push(bytes);
      callback?.();
    }),
    written,
  };
}

describe("applyDelta", () => {
  test("decodes base64 bytes and writes them to the terminal", async () => {
    const term = createTerminal();
    const delta: DeltaSlice = {
      bytes_b64: bytesToBase64(new Uint8Array([0, 65, 255])),
      head_seq: 3,
    };

    await applyDelta(term, delta);

    expect(term.write).toHaveBeenCalledOnce();
    expect(term.written).toEqual([new Uint8Array([0, 65, 255])]);
  });
});
