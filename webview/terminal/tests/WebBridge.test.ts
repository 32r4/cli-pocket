import { describe, expect, test, vi } from "vitest";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { WebBridge, type WasmClient } from "@/bridge/WebBridge";

function createClient(overrides: Partial<WasmClient> = {}): WasmClient {
  return {
    connect: vi.fn(),
    create_terminal: vi.fn(),
    send_input: vi.fn(async (_data: Uint8Array) => {}),
    resize: vi.fn(async (_cols: number, _rows: number) => {}),
    kill: vi.fn(async () => {}),
    next_event: vi.fn(async () => null),
    export_identity: vi.fn(async () => new Uint8Array([1, 2, 3])),
    import_identity: vi.fn(),
    ...overrides,
  };
}

describe("WebBridge", () => {
  test("uses a bundler-visible wasm package import", async () => {
    const source = await readFile(
      fileURLToPath(new URL("../src/bridge/WebBridge.ts", import.meta.url)),
      "utf8",
    );

    expect(source).toContain('import("cli-pocket-client-core-wasm")');
    expect(source).not.toContain("@vite-ignore");
  });

  test("serializes connect and terminal params to wasm JSON", async () => {
    const client = createClient();
    const bridge = new WebBridge(client);

    await bridge.connect({
      endpointUrl: "wss://relay.example.test/client",
      serverPublicHex: "abcd",
    });
    await bridge.createTerminal({
      cols: 80,
      rows: 24,
      cwd: "/work",
      shell: "powershell",
      env: { TERM: "xterm-256color" },
      scrollbackBytes: 4096,
    });

    expect(client.connect).toHaveBeenCalledWith(
      JSON.stringify({
        endpoint_url: "wss://relay.example.test/client",
        server_public_hex: "abcd",
        resume_token_hex: null,
      }),
    );
    expect(client.create_terminal).toHaveBeenCalledWith(
      JSON.stringify({
        cols: 80,
        rows: 24,
        cwd: "/work",
        cmd: ["powershell"],
        env: [["TERM", "xterm-256color"]],
        scrollback_bytes: 4096,
      }),
    );
  });

  test("does not require wasm create_terminal to return a terminal id", async () => {
    const bridge = new WebBridge(createClient());

    await expect(
      bridge.createTerminal({
        cols: 80,
        rows: 24,
      }),
    ).resolves.toBeUndefined();
  });

  test("adapts current active-terminal wasm methods", async () => {
    const client = createClient();
    const bridge = new WebBridge(client);
    const bytes = new Uint8Array([0, 127, 255]);

    await bridge.sendInput("terminal-1", bytes);
    await bridge.resize("terminal-1", 100, 40);
    await bridge.kill("terminal-1", "SIGTERM");

    expect(client.send_input).toHaveBeenCalledWith(bytes);
    expect(client.resize).toHaveBeenCalledWith(100, 40);
    expect(client.kill).toHaveBeenCalledWith();
  });

  test("prefers future terminal-scoped wasm command methods", async () => {
    const sendInput = vi.fn(async (_terminalId: string, _data: Uint8Array) => {});
    const resize = vi.fn(
      async (_terminalId: string, _cols: number, _rows: number) => {},
    );
    const kill = vi.fn(async (_terminalId: string, _signal: string) => {});
    const client = createClient({
      send_input: sendInput,
      resize,
      kill,
    });
    const bridge = new WebBridge(client);
    const bytes = new Uint8Array([1, 2, 3]);

    await bridge.sendInput("terminal-1", bytes);
    await bridge.resize("terminal-1", 132, 48);
    await bridge.kill("terminal-1", "SIGKILL");

    expect(sendInput).toHaveBeenCalledWith("terminal-1", bytes);
    expect(resize).toHaveBeenCalledWith("terminal-1", 132, 48);
    expect(kill).toHaveBeenCalledWith("terminal-1", "SIGKILL");
  });

  test("yields validated wasm events until the stream closes", async () => {
    const client = createClient({
      next_event: vi
        .fn()
        .mockResolvedValueOnce({ kind: "Connecting" })
        .mockResolvedValueOnce({
          kind: "Connected",
          session_id: "session-1",
        })
        .mockResolvedValueOnce(null),
    });
    const iterator = new WebBridge(client).events()[Symbol.asyncIterator]();

    await expect(iterator.next()).resolves.toEqual({
      value: { kind: "Connecting" },
      done: false,
    });
    await expect(iterator.next()).resolves.toEqual({
      value: { kind: "Connected", session_id: "session-1" },
      done: false,
    });
    await expect(iterator.next()).resolves.toEqual({
      value: undefined,
      done: true,
    });
  });

  test("rejects invalid wasm events", async () => {
    const client = createClient({
      next_event: vi.fn(async () => ({ kind: "Connected" })),
    });
    const iterator = new WebBridge(client).events()[Symbol.asyncIterator]();

    await expect(iterator.next()).rejects.toThrow("invalid ClientEvent");
  });

  test("normalizes exported identity shapes", async () => {
    await expect(
      new WebBridge(
        createClient({ export_identity: vi.fn(async () => [1, 2, 255]) }),
      ).exportIdentity(),
    ).resolves.toEqual(new Uint8Array([1, 2, 255]));

    await expect(
      new WebBridge(
        createClient({ export_identity: vi.fn(async () => "identity") }),
      ).exportIdentity(),
    ).resolves.toEqual(new TextEncoder().encode("identity"));
  });
});
