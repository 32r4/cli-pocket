import { describe, expect, test } from "vitest";

import { MockBridge } from "@/bridge/MockBridge";

describe("MockBridge", () => {
  test("emits connection events and creates a terminal with welcome output", async () => {
    const bridge = new MockBridge();
    const iterator = bridge.events()[Symbol.asyncIterator]();

    await bridge.connect({
      endpointUrl: "mock://relay",
      serverPublicHex: "server",
    });

    await expect(iterator.next()).resolves.toEqual({
      value: { kind: "Connecting" },
      done: false,
    });
    await expect(iterator.next()).resolves.toEqual({
      value: { kind: "Connected", session_id: "mock-session" },
      done: false,
    });

    await bridge.createTerminal({ cols: 100, rows: 32 });

    const created = await iterator.next();
    expect(created.done).toBe(false);
    expect(created.value).toEqual({
      kind: "TerminalCreated",
      info: {
        terminal: "mock-terminal",
        cols: 100,
        rows: 32,
        created_at_unix_ms: expect.any(Number),
        label: "Mock terminal",
        attached_clients: 1,
      },
    });

    await expect(iterator.next()).resolves.toEqual({
      value: {
        kind: "TerminalOutput",
        terminal_id: "mock-terminal",
        stream_seq: 1,
        bytes_b64: btoa("Welcome to cli-pocket mock terminal\r\n$ "),
      },
      done: false,
    });
  });

  test("echoes input for the active terminal and closes pending iterators", async () => {
    const bridge = new MockBridge();
    const iterator = bridge.events()[Symbol.asyncIterator]();
    await bridge.connect({
      endpointUrl: "mock://relay",
      serverPublicHex: "server",
    });
    await iterator.next();
    await iterator.next();
    await bridge.createTerminal({ cols: 80, rows: 24 });
    await iterator.next();
    await iterator.next();

    await bridge.sendInput("other-terminal", new TextEncoder().encode("ignored"));
    await bridge.sendInput("mock-terminal", new TextEncoder().encode("pwd\r"));

    await expect(iterator.next()).resolves.toEqual({
      value: {
        kind: "TerminalOutput",
        terminal_id: "mock-terminal",
        stream_seq: 2,
        bytes_b64: btoa("pwd\r\n$ "),
      },
      done: false,
    });

    const pending = iterator.next();
    await bridge.close();
    await expect(pending).resolves.toEqual({ value: undefined, done: true });
    await expect(iterator.next()).resolves.toEqual({
      value: undefined,
      done: true,
    });
  });

  test("encodes large terminal output without exceeding argument limits", async () => {
    const bridge = new MockBridge();
    const iterator = bridge.events()[Symbol.asyncIterator]();
    await bridge.connect({
      endpointUrl: "mock://relay",
      serverPublicHex: "server",
    });
    await iterator.next();
    await iterator.next();
    await bridge.createTerminal({ cols: 80, rows: 24 });
    await iterator.next();
    await iterator.next();

    const largePaste = "x".repeat(1_000_000);

    await expect(
      bridge.sendInput("mock-terminal", new TextEncoder().encode(`${largePaste}\r`)),
    ).resolves.toBeUndefined();

    const output = await iterator.next();
    expect(output.done).toBe(false);
    expect(output.value).toEqual({
      kind: "TerminalOutput",
      terminal_id: "mock-terminal",
      stream_seq: 2,
      bytes_b64: btoa(`${largePaste}\r\n$ `),
    });
  });
});
