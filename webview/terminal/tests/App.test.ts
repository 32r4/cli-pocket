import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { App } from "@/ui/App";
import type { ClientBridge } from "@/bridge/ClientBridge";
import type { ClientEvent } from "@/types/events";
import type { DeltaSlice } from "@/types/frame";

interface Disposable {
  dispose(): void;
}

interface MockRenderer {
  mount: Mock<[container: HTMLElement], void>;
  onData: Mock<[(data: string) => void], Disposable>;
  onResize: Mock<[(cols: number, rows: number) => void], Disposable>;
  resize: Mock<[cols: number, rows: number], void>;
  proposeDimensions: Mock<[], { cols: number; rows: number } | undefined>;
  applyDelta: Mock<[delta: DeltaSlice], Promise<void>>;
  dispose: Mock<[], void>;
}

class FakeElement {
  readonly children: FakeElement[] = [];
  readonly dataset: Record<string, string> = {};
  readonly listeners = new Map<string, Array<() => void>>();
  className = "";
  textContent: string | null = null;
  type = "";
  private readonly attributes = new Set<string>();

  constructor(readonly tagName: string) {}

  append(...children: FakeElement[]): void {
    this.children.push(...children);
  }

  replaceChildren(...children: FakeElement[]): void {
    this.children.splice(0, this.children.length, ...children);
  }

  addEventListener(event: string, handler: () => void): void {
    const handlers = this.listeners.get(event) ?? [];
    handlers.push(handler);
    this.listeners.set(event, handlers);
  }

  click(): void {
    for (const handler of this.listeners.get("click") ?? []) {
      handler();
    }
  }

  toggleAttribute(attribute: string, force: boolean): void {
    if (force) {
      this.attributes.add(attribute);
      return;
    }
    this.attributes.delete(attribute);
  }

  querySelector(selector: string): FakeElement | null {
    return findElement(this, selector);
  }
}

class EventQueue {
  private readonly queued: ClientEvent[] = [];
  private readonly waiters: Array<(result: IteratorResult<ClientEvent>) => void> = [];
  private closed = false;

  push(event: ClientEvent): void {
    const waiter = this.waiters.shift();
    if (waiter !== undefined) {
      waiter({ value: event, done: false });
      return;
    }
    this.queued.push(event);
  }

  close(): void {
    this.closed = true;
    const waiters = this.waiters.splice(0);
    for (const waiter of waiters) {
      waiter({ value: undefined, done: true });
    }
  }

  events(): AsyncIterable<ClientEvent> {
    return {
      [Symbol.asyncIterator]: () => ({
        next: async (): Promise<IteratorResult<ClientEvent>> => {
          if (this.closed) {
            return { value: undefined, done: true };
          }

          const event = this.queued.shift();
          if (event !== undefined) {
            return { value: event, done: false };
          }

          return new Promise((resolve) => {
            this.waiters.push(resolve);
          });
        },
      }),
    };
  }
}

const rendererInstances: MockRenderer[] = [];

vi.mock("@/render/Renderer", () => ({
  Renderer: vi.fn((): MockRenderer => {
    const renderer: MockRenderer = {
      mount: vi.fn(),
      onData: vi.fn<[handler: (data: string) => void], Disposable>(() =>
        createDisposable(),
      ),
      onResize: vi.fn<
        [handler: (cols: number, rows: number) => void],
        Disposable
      >(() => createDisposable()),
      resize: vi.fn(),
      proposeDimensions: vi.fn((): { cols: number; rows: number } | undefined => ({
        cols: 90,
        rows: 28,
      })),
      applyDelta: vi.fn<[delta: DeltaSlice], Promise<void>>(async () => undefined),
      dispose: vi.fn(),
    };
    rendererInstances.push(renderer);
    return renderer;
  }),
}));

function createBridge(queue: EventQueue): ClientBridge {
  return {
    connect: vi.fn(async () => undefined),
    events: vi.fn(() => queue.events()),
    createTerminal: vi.fn(async () => undefined),
    sendInput: vi.fn(async () => undefined),
    resize: vi.fn(async () => undefined),
    kill: vi.fn(async () => undefined),
    exportIdentity: vi.fn(async () => new Uint8Array()),
    importIdentity: vi.fn(async () => undefined),
    close: vi.fn(async () => {
      queue.close();
    }),
  };
}

describe("App", () => {
  beforeEach(() => {
    rendererInstances.length = 0;
    vi.stubGlobal("document", {
      body: new FakeElement("body"),
      createElement: (tagName: string) => new FakeElement(tagName),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("requests one terminal after connecting, routes events and input by active terminal, and cleans up", async () => {
    const queue = new EventQueue();
    const bridge = createBridge(queue);
    const host = document.createElement("div");
    const app = new App(host, bridge, "tauri");

    await app.start();

    const renderer = rendererInstances[0];
    expect(renderer?.mount).toHaveBeenCalledWith(host.querySelector(".terminal-screen"));
    expect(bridge.createTerminal).not.toHaveBeenCalled();
    expect(host.querySelector(".virtual-key-bar")).not.toBeNull();

    queue.push({ kind: "Connected", session_id: "session-1" });
    await vi.waitFor(() => {
      expect(bridge.createTerminal).toHaveBeenCalledWith({ cols: 90, rows: 28 });
    });

    queue.push({ kind: "Connected", session_id: "session-1" });
    await Promise.resolve();
    expect(bridge.createTerminal).toHaveBeenCalledOnce();

    renderer?.onData.mock.calls[0]?.[0]("early");
    renderer?.onResize.mock.calls[0]?.[0](100, 30);

    expect(bridge.sendInput).not.toHaveBeenCalled();
    expect(bridge.resize).not.toHaveBeenCalled();

    queue.push({
      kind: "TerminalCreated",
      info: {
        terminal: "term-1",
        cols: 90,
        rows: 28,
        created_at_unix_ms: 1,
        label: null,
        attached_clients: 1,
      },
    });
    await vi.waitFor(() => {
      expect(host.querySelector(".status-bar__item--terminal")?.textContent).toBe("term-1");
    });

    renderer?.onData.mock.calls[0]?.[0]("go");
    renderer?.onResize.mock.calls[0]?.[0](120, 40);
    queue.push({
      kind: "TerminalOutput",
      terminal_id: "term-1",
      stream_seq: 7,
      bytes_b64: btoa("ok"),
    });

    await vi.waitFor(() => {
      expect(bridge.sendInput).toHaveBeenCalledWith(
        "term-1",
        new TextEncoder().encode("go"),
      );
      expect(bridge.resize).toHaveBeenCalledWith("term-1", 120, 40);
      expect(renderer?.applyDelta).toHaveBeenCalledWith({
        bytes_b64: btoa("ok"),
        head_seq: 7,
      });
    });

    const ctrlC = host.querySelector<HTMLButtonElement>("[data-virtual-key='Ctrl+C']");
    ctrlC?.click();

    await vi.waitFor(() => {
      expect(bridge.sendInput).toHaveBeenCalledWith("term-1", new Uint8Array([0x03]));
    });

    queue.push({
      kind: "TerminalExited",
      terminal_id: "term-1",
      info: { code: 0, signal: null, at_unix_ms: 2 },
    });
    await vi.waitFor(() => {
      expect(host.querySelector(".status-bar__item--terminal")?.textContent).toBe("none");
    });

    await app.dispose();

    expect(renderer?.dispose).toHaveBeenCalledOnce();
    expect(bridge.close).toHaveBeenCalledOnce();
  });

  test("surfaces terminal creation failures while remaining disposable", async () => {
    const queue = new EventQueue();
    const bridge = createBridge(queue);
    vi.mocked(bridge.createTerminal).mockRejectedValueOnce(new Error("spawn failed"));
    const host = document.createElement("div");
    const app = new App(host, bridge, "web");

    await expect(app.start()).resolves.toBeUndefined();

    queue.push({ kind: "Connected", session_id: "session-1" });
    await vi.waitFor(() => {
      expect(host.querySelector(".status-bar__item--error")?.textContent).toBe(
        "spawn failed",
      );
    });

    await expect(app.dispose()).resolves.toBeUndefined();
    expect(bridge.close).toHaveBeenCalledOnce();
  });
});

function createDisposable(): Disposable {
  return { dispose: vi.fn() };
}

function findElement(root: FakeElement, selector: string): FakeElement | null {
  for (const child of root.children) {
    if (matchesSelector(child, selector)) {
      return child;
    }

    const nested = findElement(child, selector);
    if (nested !== null) {
      return nested;
    }
  }

  return null;
}

function matchesSelector(element: FakeElement, selector: string): boolean {
  if (selector.startsWith(".")) {
    return element.className.split(" ").includes(selector.slice(1));
  }

  const dataVirtualKey = selector.match(/^\[data-virtual-key='(.+)'\]$/);
  return dataVirtualKey !== null && element.dataset["virtualKey"] === dataVirtualKey[1];
}
