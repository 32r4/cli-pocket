import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import type { ClientBridge, ConnectConfig } from "@/bridge/ClientBridge";
import type { ClientEvent } from "@/types/events";
import type { TerminalId } from "@/types/frame";

interface WindowStub {
  location: { search: string };
  addEventListener: (event: string, handler: () => void) => void;
}

interface DocumentStub {
  getElementById: (id: string) => HTMLElement | null;
}

interface AppInstance {
  start: ReturnType<typeof vi.fn<[], Promise<void>>>;
  dispose: ReturnType<typeof vi.fn<[], Promise<void>>>;
  showError: ReturnType<typeof vi.fn<[message: string], void>>;
}

const appInstances: AppInstance[] = [];
const webBridge = createBridge();
const mockBridgeInstances: ClientBridge[] = [];
const root = {} as HTMLElement;

vi.mock("@/ui/App", () => ({
  App: vi.fn((): AppInstance => {
    const app: AppInstance = {
      start: vi.fn<[], Promise<void>>(async () => undefined),
      dispose: vi.fn<[], Promise<void>>(async () => undefined),
      showError: vi.fn(),
    };
    appInstances.push(app);
    return app;
  }),
}));

vi.mock("@/bridge/WebBridge", () => ({
  WebBridge: {
    create: vi.fn(() => webBridge),
  },
}));

vi.mock("@/bridge/TauriBridge", () => ({
  TauriBridge: vi.fn(() => createBridge()),
}));

vi.mock("@/bridge/MockBridge", () => ({
  MockBridge: vi.fn(() => {
    const bridge = createBridge();
    mockBridgeInstances.push(bridge);
    return bridge;
  }),
}));

describe("main bootstrap", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    appInstances.length = 0;
    mockBridgeInstances.length = 0;
    vi.stubGlobal("document", {
      getElementById: (id: string) => (id === "app" ? root : null),
    } satisfies DocumentStub);
    vi.stubGlobal("window", {
      location: { search: "" },
      addEventListener: vi.fn(),
    } satisfies WindowStub);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("connects after startup when URL connection parameters are present", async () => {
    setSearch(
      "?endpointUrl=wss%3A%2F%2Frelay.example%2Fclient&serverPublicHex=abcd&resumeTokenHex=1234",
    );

    await import("@/main");

    expect(appInstances[0]?.start).toHaveBeenCalledOnce();
    expect(webBridge.connect).toHaveBeenCalledWith({
      endpointUrl: "wss://relay.example/client",
      serverPublicHex: "abcd",
      resumeTokenHex: "1234",
    });
    expect(appInstances[0]?.showError).not.toHaveBeenCalled();
  });

  test("supports snake_case URL connection parameters", async () => {
    setSearch(
      "?endpoint_url=wss%3A%2F%2Frelay.example%2Fclient&server_public_hex=abcd&resume_token_hex=1234",
    );

    await import("@/main");

    expect(webBridge.connect).toHaveBeenCalledWith({
      endpointUrl: "wss://relay.example/client",
      serverPublicHex: "abcd",
      resumeTokenHex: "1234",
    });
  });

  test("mounts without connecting when URL connection parameters are absent", async () => {
    await import("@/main");

    expect(appInstances[0]?.start).toHaveBeenCalledOnce();
    expect(webBridge.connect).not.toHaveBeenCalled();
  });

  test("keeps mock mode connected without URL connection parameters", async () => {
    setSearch("?mock=1");

    await import("@/main");

    expect(appInstances[0]?.start).toHaveBeenCalledOnce();
    expect(mockBridgeInstances[0]?.connect).toHaveBeenCalledWith({
      endpointUrl: "mock://cli-pocket",
      serverPublicHex: "mock",
    });
    expect(webBridge.connect).not.toHaveBeenCalled();
  });

  test("mock mode ignores URL connection parameters after mock connect", async () => {
    setSearch(
      "?mock=1&endpointUrl=wss%3A%2F%2Frelay.example%2Fclient&serverPublicHex=abcd",
    );

    await import("@/main");

    expect(appInstances[0]?.start).toHaveBeenCalledOnce();
    expect(mockBridgeInstances[0]?.connect).toHaveBeenCalledOnce();
    expect(mockBridgeInstances[0]?.connect).toHaveBeenCalledWith({
      endpointUrl: "mock://cli-pocket",
      serverPublicHex: "mock",
    });
    expect(webBridge.connect).not.toHaveBeenCalled();
  });
});

function setSearch(search: string): void {
  const stub = window as unknown as WindowStub;
  stub.location.search = search;
}

function createBridge(): ClientBridge {
  return {
    connect: vi.fn<[config: ConnectConfig], Promise<void>>(async () => undefined),
    events: vi.fn<[], AsyncIterable<ClientEvent>>(() => ({
      [Symbol.asyncIterator]: () => ({
        next: async (): Promise<IteratorResult<ClientEvent>> => ({
          value: undefined,
          done: true,
        }),
      }),
    })),
    createTerminal: vi.fn(async () => undefined),
    sendInput: vi.fn(
      async (_terminalId: TerminalId, _bytes: Uint8Array) => undefined,
    ),
    resize: vi.fn(
      async (_terminalId: TerminalId, _cols: number, _rows: number) => undefined,
    ),
    kill: vi.fn(async (_terminalId: TerminalId, _signal: string) => undefined),
    exportIdentity: vi.fn(async () => new Uint8Array()),
    importIdentity: vi.fn(async (_blob: Uint8Array) => undefined),
    close: vi.fn(async () => undefined),
  };
}
