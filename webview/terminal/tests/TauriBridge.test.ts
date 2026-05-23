import { beforeEach, describe, expect, test, vi } from "vitest";

import { TauriBridge } from "@/bridge/TauriBridge";
import type { ClientEvent } from "@/types/events";

type ListenHandler = (event: { payload: ClientEvent }) => void;
type InvokeMock = (command: string, args?: unknown) => Promise<unknown>;
type ListenMock = (
  channel: string,
  handler: ListenHandler,
) => Promise<() => void>;

const invoke = vi.fn<Parameters<InvokeMock>, ReturnType<InvokeMock>>();
const listen = vi.fn<Parameters<ListenMock>, ReturnType<ListenMock>>();

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

describe("TauriBridge", () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  test("invokes centralized Tauri command names with expected payloads", async () => {
    invoke.mockResolvedValueOnce(undefined);
    invoke.mockResolvedValueOnce("terminal-1");
    invoke.mockResolvedValueOnce(undefined);
    invoke.mockResolvedValueOnce(undefined);
    invoke.mockResolvedValueOnce(undefined);
    invoke.mockResolvedValueOnce([1, 2, 255]);
    invoke.mockResolvedValueOnce(undefined);

    const bridge = new TauriBridge();

    await bridge.connect({
      endpointUrl: "wss://example.test",
      serverPublicHex: "abcd",
      resumeTokenHex: "1234",
    });
    await expect(
      bridge.createTerminal({
        cols: 80,
        rows: 24,
        cwd: "/work",
        cmd: ["pwsh"],
        shell: "powershell",
        env: { TERM: "xterm-256color" },
        scrollbackBytes: 4096,
      }),
    ).resolves.toBe("terminal-1");
    await bridge.sendInput("terminal-1", new Uint8Array([0, 127, 255]));
    await bridge.resize("terminal-1", 100, 40);
    await bridge.kill("terminal-1", "SIGTERM");
    await expect(bridge.exportIdentity()).resolves.toEqual(
      new Uint8Array([1, 2, 255]),
    );
    await bridge.importIdentity(new Uint8Array([4, 5, 6]));

    expect(invoke.mock.calls).toEqual([
      [
        "cli_pocket_connect",
        {
          config: {
            endpointUrl: "wss://example.test",
            serverPublicHex: "abcd",
            resumeTokenHex: "1234",
          },
        },
      ],
      [
        "cli_pocket_create_terminal",
        {
          params: {
            cols: 80,
            rows: 24,
            cwd: "/work",
            cmd: ["pwsh"],
            shell: "powershell",
            env: { TERM: "xterm-256color" },
            scrollbackBytes: 4096,
          },
        },
      ],
      [
        "cli_pocket_send_input",
        { terminalId: "terminal-1", bytes: [0, 127, 255] },
      ],
      [
        "cli_pocket_resize",
        { terminalId: "terminal-1", cols: 100, rows: 40 },
      ],
      ["cli_pocket_kill", { terminalId: "terminal-1", signal: "SIGTERM" }],
      ["cli_pocket_export_identity", undefined],
      ["cli_pocket_import_identity", { blob: [4, 5, 6] }],
    ]);
  });

  test("buffers Tauri events for async iteration and unregisters on close", async () => {
    let handler: ListenHandler | undefined;
    const unlisten = vi.fn();
    listen.mockImplementationOnce(async (channel, nextHandler) => {
      expect(channel).toBe("cli_pocket:event");
      handler = nextHandler;
      return unlisten;
    });
    invoke.mockResolvedValueOnce(undefined);

    const bridge = new TauriBridge();
    const iterator = bridge.events()[Symbol.asyncIterator]();
    const first = iterator.next();

    await vi.waitFor(() => {
      expect(handler).toBeDefined();
    });

    handler?.({ payload: { kind: "Connecting" } });
    handler?.({ payload: { kind: "Connected", session_id: "session-1" } });

    await expect(first).resolves.toEqual({
      value: { kind: "Connecting" },
      done: false,
    });
    await expect(iterator.next()).resolves.toEqual({
      value: { kind: "Connected", session_id: "session-1" },
      done: false,
    });

    await bridge.close();

    expect(unlisten).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("cli_pocket_close", undefined);
    await expect(iterator.next()).resolves.toEqual({
      value: undefined,
      done: true,
    });
  });

  test("does not leak a listener when closed immediately after events starts", async () => {
    const unlisten = vi.fn();
    invoke.mockResolvedValueOnce(undefined);

    const bridge = new TauriBridge();
    const iterator = bridge.events()[Symbol.asyncIterator]();

    await bridge.close();

    expect(listen).not.toHaveBeenCalled();
    expect(unlisten).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("cli_pocket_close", undefined);
    await expect(iterator.next()).resolves.toEqual({
      value: undefined,
      done: true,
    });
  });

  test("surfaces listener setup failures to pending and future iterator next calls", async () => {
    const setupError = new Error("listen failed");
    listen.mockRejectedValueOnce(setupError);

    const bridge = new TauriBridge();
    const iterator = bridge.events()[Symbol.asyncIterator]();

    await expect(iterator.next()).rejects.toBe(setupError);
    await expect(iterator.next()).rejects.toBe(setupError);
  });
});
