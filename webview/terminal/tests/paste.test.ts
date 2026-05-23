import { afterEach, describe, expect, test, vi } from "vitest";

import { getClipboardText, wrapBracketedPaste } from "@/input/paste";

const originalNavigator = globalThis.navigator;

describe("getClipboardText", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    if (originalNavigator !== undefined) {
      vi.stubGlobal("navigator", originalNavigator);
    }
  });

  test("returns empty text when navigator is unavailable", async () => {
    vi.stubGlobal("navigator", undefined);

    await expect(getClipboardText()).resolves.toBe("");
  });

  test("returns empty text when clipboard readText is unavailable", async () => {
    vi.stubGlobal("navigator", {});

    await expect(getClipboardText()).resolves.toBe("");
  });

  test("reads text from navigator clipboard when available", async () => {
    vi.stubGlobal("navigator", {
      clipboard: {
        readText: vi.fn<[], Promise<string>>().mockResolvedValue("copied text"),
      },
    });

    await expect(getClipboardText()).resolves.toBe("copied text");
  });

  test("returns empty text when clipboard read fails", async () => {
    vi.stubGlobal("navigator", {
      clipboard: {
        readText: vi.fn<[], Promise<string>>().mockRejectedValue(new Error("denied")),
      },
    });

    await expect(getClipboardText()).resolves.toBe("");
  });
});

describe("wrapBracketedPaste", () => {
  test("wraps text in bracketed paste control sequences", () => {
    const bytes = wrapBracketedPaste("hello");

    expect([...bytes]).toEqual([
      ...new TextEncoder().encode("\u001b[200~"),
      ...new TextEncoder().encode("hello"),
      ...new TextEncoder().encode("\u001b[201~"),
    ]);
  });

  test("encodes pasted text as UTF-8", () => {
    expect([...wrapBracketedPaste("\u2713")]).toEqual([
      0x1b,
      0x5b,
      0x32,
      0x30,
      0x30,
      0x7e,
      0xe2,
      0x9c,
      0x93,
      0x1b,
      0x5b,
      0x32,
      0x30,
      0x31,
      0x7e,
    ]);
  });
});
