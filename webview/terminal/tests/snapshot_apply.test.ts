import { describe, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { applySnapshot } from "@/render/snapshot";
import type { AnchorState, Snapshot } from "@/types/frame";

interface WriteCall {
  bytes: Uint8Array;
  text: string;
}

interface FakeTerminal {
  reset: Mock<[], void>;
  resize: Mock<[cols: number, rows: number], void>;
  write: Mock<[bytes: Uint8Array, callback?: (() => void) | undefined], void>;
  calls: WriteCall[];
}

const defaultAnchor: AnchorState = {
  cursor: [0, 0],
  sgr: {
    fg: null,
    bg: null,
    bold: false,
    faint: false,
    italic: false,
    underline: false,
    blink: false,
    reverse: false,
    strikethrough: false,
  },
  modes: {
    deccmm_cursor_keys: false,
    autowrap: true,
    alt_screen: false,
    bracketed_paste: false,
    mouse_reporting: "Off",
    origin_mode: false,
  },
  charset: {
    g: [66, 66, 66, 66],
    gl: 0,
    gr: 0,
  },
  title: null,
};

function bytesToBase64(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes));
}

function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    cols: 80,
    rows: 24,
    anchor_state: defaultAnchor,
    bytes_b64: bytesToBase64(new TextEncoder().encode("hello")),
    head_seq: 5,
    ...overrides,
  };
}

function createTerminal(): FakeTerminal {
  const calls: WriteCall[] = [];
  return {
    reset: vi.fn(),
    resize: vi.fn(),
    write: vi.fn((bytes: Uint8Array, callback?: () => void) => {
      calls.push({ bytes, text: new TextDecoder().decode(bytes) });
      callback?.();
    }),
    calls,
  };
}

describe("applySnapshot", () => {
  test("resets, resizes, restores default anchor, and writes snapshot bytes", async () => {
    const term = createTerminal();

    await applySnapshot(term, snapshot());

    expect(term.resize).toHaveBeenCalledWith(80, 24);
    expect(term.reset.mock.invocationCallOrder[0]).toBeLessThan(
      term.resize.mock.invocationCallOrder[0],
    );
    expect(term.resize.mock.invocationCallOrder[0]).toBeLessThan(
      term.write.mock.invocationCallOrder[0],
    );
    expect(term.calls).toEqual([
      {
        bytes: new TextEncoder().encode("\x1b[0m\x1b(B\x1b)B\x1b*B\x1b+B\x0f\x1b[1;1H"),
        text: "\x1b[0m\x1b(B\x1b)B\x1b*B\x1b+B\x0f\x1b[1;1H",
      },
      { bytes: new TextEncoder().encode("hello"), text: "hello" },
    ]);
  });

  test("restores title and modes before replaying snapshot bytes", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          modes: {
            ...defaultAnchor.modes,
            deccmm_cursor_keys: true,
            autowrap: false,
            alt_screen: true,
            bracketed_paste: true,
            mouse_reporting: "ButtonEvent",
            origin_mode: true,
          },
          title: "shell",
        },
      }),
    );

    expect(term.calls.map((call) => call.text)).toEqual([
      "\x1b]0;shell\x07\x1b[?1h\x1b[?7l\x1b[?1049h\x1b[?2004h\x1b[?1002h\x1b[?6h\x1b[0m\x1b(B\x1b)B\x1b*B\x1b+B\x0f\x1b[1;1H",
      "hello",
    ]);
  });

  test("restores cursor using one-based terminal coordinates", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          cursor: [11, 6],
        },
      }),
    );

    expect(term.calls[0]?.text).toContain("\x1b[7;12H");
  });

  test("restores SGR attributes and color variants", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          sgr: {
            fg: { Rgb: [12, 34, 56] },
            bg: { Indexed: 202 },
            bold: true,
            faint: true,
            italic: true,
            underline: true,
            blink: true,
            reverse: true,
            strikethrough: true,
          },
        },
      }),
    );

    expect(term.calls[0]?.text).toContain(
      "\x1b[0m\x1b[1;2;3;4;5;7;9;38;2;12;34;56;48;5;202m",
    );
  });

  test("maps palette colors to standard ANSI SGR ranges", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          sgr: {
            ...defaultAnchor.sgr,
            fg: { Palette: 14 },
            bg: { Palette: 3 },
          },
        },
      }),
    );

    expect(term.calls[0]?.text).toContain("\x1b[0m\x1b[96;43m");
  });

  test("restores charset designations and safe GL shifts", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          charset: {
            g: [48, 65, 66, 85],
            gl: 1,
            gr: 2,
          },
        },
      }),
    );

    expect(term.calls[0]?.text).toContain("\x1b(0\x1b)A\x1b*B\x1b+U\x0e");
    expect(term.calls[0]?.text).not.toContain("\x1b~");
  });

  test("maps X10 mouse reporting to DECSET 9", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          modes: {
            ...defaultAnchor.modes,
            mouse_reporting: "X10",
          },
        },
      }),
    );

    expect(term.calls[0]?.text).toContain("\x1b[?9h");
    expect(term.calls[0]?.text).not.toContain("\x1b[?1000h");
  });

  test("sanitizes title controls before embedding OSC title", async () => {
    const term = createTerminal();

    await applySnapshot(
      term,
      snapshot({
        anchor_state: {
          ...defaultAnchor,
          title: "safe\x07\x1b[?1003h\x1b\\name\x9dmore\x9c",
        },
      }),
    );

    expect(term.calls[0]?.text.startsWith("\x1b]0;safe[?1003h\\namemore\x07")).toBe(
      true,
    );
    expect(term.calls[0]?.text).not.toContain("\x1b[?1003h");
    expect(term.calls[0]?.text.indexOf("\x07")).toBe(
      "\x1b]0;safe[?1003h\\namemore\x07".length - 1,
    );
  });
});
