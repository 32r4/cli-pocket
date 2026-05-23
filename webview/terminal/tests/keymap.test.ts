import { describe, expect, test } from "vitest";

import { virtualKeyToBytes } from "@/input/keymap";

describe("virtualKeyToBytes", () => {
  test.each([
    ["Esc", [0x1b]],
    ["Tab", [0x09]],
    ["ArrowUp", [0x1b, 0x5b, 0x41]],
    ["ArrowDown", [0x1b, 0x5b, 0x42]],
    ["ArrowRight", [0x1b, 0x5b, 0x43]],
    ["ArrowLeft", [0x1b, 0x5b, 0x44]],
    ["Home", [0x1b, 0x5b, 0x48]],
    ["End", [0x1b, 0x5b, 0x46]],
    ["PageUp", [0x1b, 0x5b, 0x35, 0x7e]],
    ["PageDown", [0x1b, 0x5b, 0x36, 0x7e]],
    ["Pipe", [0x7c]],
    ["Tilde", [0x7e]],
  ] as const)("maps %s to terminal bytes", (key, bytes) => {
    expect([...virtualKeyToBytes(key)]).toEqual(bytes);
  });

  test.each([
    ["Ctrl+C", 0x03],
    ["Ctrl+D", 0x04],
    ["Ctrl+Z", 0x1a],
    ["Ctrl+L", 0x0c],
    ["Ctrl+R", 0x12],
    ["Ctrl+U", 0x15],
    ["Ctrl+W", 0x17],
  ] as const)("maps %s to its control byte", (key, byte) => {
    expect(virtualKeyToBytes(key)).toEqual(new Uint8Array([byte]));
  });
});
