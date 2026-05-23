import { beforeEach, describe, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { Renderer } from "@/render/Renderer";
import type { DeltaSlice, Snapshot } from "@/types/frame";

interface Disposable {
  dispose(): void;
}

interface MockTerminal {
  open: Mock<[container: HTMLElement], void>;
  focus: Mock<[], void>;
  loadAddon: Mock<[addon: Disposable], void>;
  onData: Mock<[(data: string) => void], Disposable>;
  onResize: Mock<[(size: { cols: number; rows: number }) => void], Disposable>;
  write: Mock<[bytes: Uint8Array, callback?: (() => void) | undefined], void>;
  reset: Mock<[], void>;
  resize: Mock<[cols: number, rows: number], void>;
  dispose: Mock<[], void>;
  unicode: {
    activeVersion: string;
  };
}

interface MockFitAddon extends Disposable {
  fit: Mock<[], void>;
  proposeDimensions: Mock<[], Dimensions | undefined>;
}

interface MockWebglAddon extends Disposable {
  onContextLoss: Mock<[handler: () => void], Disposable>;
}

interface Dimensions {
  cols: number;
  rows: number;
}

const terminalInstances: MockTerminal[] = [];
const fitAddonInstances: MockFitAddon[] = [];
const webglAddonInstances: MockWebglAddon[] = [];
const unicodeAddonInstances: Disposable[] = [];
const webglOptions = vi.hoisted(() => ({
  throwOnCreate: false,
  throwOnLoadAddon: false,
}));

const createDisposable = (): Disposable => ({ dispose: vi.fn() });

vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn((): MockTerminal => {
    const terminal: MockTerminal = {
      open: vi.fn(),
      focus: vi.fn(),
      loadAddon: vi.fn((addon: Disposable) => {
        if (webglOptions.throwOnLoadAddon && webglAddonInstances.includes(addon as MockWebglAddon)) {
          throw new Error("failed to load webgl addon");
        }
      }),
      onData: vi.fn((_handler: (data: string) => void) => createDisposable()),
      onResize: vi.fn((_handler: (size: Dimensions) => void) => createDisposable()),
      write: vi.fn((_bytes, callback) => callback?.()),
      reset: vi.fn(),
      resize: vi.fn(),
      dispose: vi.fn(),
      unicode: {
        activeVersion: "",
      },
    };
    terminalInstances.push(terminal);
    return terminal;
  }),
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn((): MockFitAddon => {
    const addon: MockFitAddon = {
      fit: vi.fn(),
      proposeDimensions: vi.fn((): Dimensions | undefined => ({ cols: 100, rows: 30 })),
      dispose: vi.fn(),
    };
    fitAddonInstances.push(addon);
    return addon;
  }),
}));

vi.mock("@xterm/addon-unicode11", () => ({
  Unicode11Addon: vi.fn((): Disposable => {
    const addon = createDisposable();
    unicodeAddonInstances.push(addon);
    return addon;
  }),
}));

vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: vi.fn((): MockWebglAddon => {
    if (webglOptions.throwOnCreate) {
      throw new Error("webgl unavailable");
    }

    const addon: MockWebglAddon = {
      onContextLoss: vi.fn((_handler: () => void): Disposable => createDisposable()),
      dispose: vi.fn(),
    };
    webglAddonInstances.push(addon);
    return addon;
  }),
}));

vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

class MockResizeObserver {
  readonly observe = vi.fn();
  readonly disconnect = vi.fn();
  readonly unobserve = vi.fn();

  constructor(private readonly callback: ResizeObserverCallback) {}

  emit(): void {
    this.callback([], this);
  }
}

const resizeObservers: MockResizeObserver[] = [];

vi.stubGlobal(
  "ResizeObserver",
  vi.fn((callback: ResizeObserverCallback) => {
    const observer = new MockResizeObserver(callback);
    resizeObservers.push(observer);
    return observer;
  }),
);

function snapshot(): Snapshot {
  return {
    cols: 80,
    rows: 24,
    anchor_state: {
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
    },
    bytes_b64: btoa("snapshot"),
    head_seq: 1,
  };
}

describe("Renderer", () => {
  beforeEach(() => {
    terminalInstances.length = 0;
    fitAddonInstances.length = 0;
    webglAddonInstances.length = 0;
    unicodeAddonInstances.length = 0;
    resizeObservers.length = 0;
    webglOptions.throwOnCreate = false;
    webglOptions.throwOnLoadAddon = false;
  });

  test("mounts xterm addons, fits on resize, and reuses the same container", () => {
    const renderer = new Renderer();
    const container = {} as HTMLElement;

    renderer.mount(container);

    const terminal = terminalInstances[0];
    const fitAddon = fitAddonInstances[0];
    const webglAddon = webglAddonInstances[0];
    expect(renderer.terminal).toBe(terminal);
    expect(terminal?.loadAddon).toHaveBeenCalledWith(fitAddon);
    expect(terminal?.loadAddon).toHaveBeenCalledWith(unicodeAddonInstances[0]);
    expect(terminal?.loadAddon).toHaveBeenCalledWith(webglAddon);
    expect(terminal?.unicode.activeVersion).toBe("11");
    expect(terminal?.open).toHaveBeenCalledWith(container);
    expect(terminal?.focus).toHaveBeenCalledTimes(1);
    expect(fitAddon?.fit).toHaveBeenCalledTimes(1);

    resizeObservers[0]?.emit();

    expect(fitAddon?.fit).toHaveBeenCalledTimes(2);

    renderer.mount(container);

    expect(terminal?.open).toHaveBeenCalledTimes(1);
    expect(resizeObservers[0]?.disconnect).not.toHaveBeenCalled();
    expect(resizeObservers).toHaveLength(1);
    expect(fitAddon?.fit).toHaveBeenCalledTimes(3);

    renderer.dispose();

    expect(resizeObservers[0]?.disconnect).toHaveBeenCalledTimes(1);
    expect(terminal?.dispose).toHaveBeenCalledTimes(1);
  });

  test("rejects mounting a second container before dispose", () => {
    const renderer = new Renderer();
    const firstContainer = {} as HTMLElement;
    const secondContainer = {} as HTMLElement;

    renderer.mount(firstContainer);

    expect(() => renderer.mount(secondContainer)).toThrowError(
      "Renderer is already mounted to a different container",
    );
    expect(terminalInstances[0]?.open).toHaveBeenCalledTimes(1);
    expect(terminalInstances[0]?.open).toHaveBeenCalledWith(firstContainer);
    expect(resizeObservers).toHaveLength(1);
  });

  test("rejects mounting after dispose", () => {
    const renderer = new Renderer();
    const container = {} as HTMLElement;

    renderer.mount(container);
    renderer.dispose();

    expect(() => renderer.mount(container)).toThrowError("Renderer has been disposed");
    expect(terminalInstances[0]?.open).toHaveBeenCalledTimes(1);
  });

  test("returns xterm event disposables and delegates dimensions and frame application", async () => {
    const renderer = new Renderer();
    const dataHandler = vi.fn();
    const resizeHandler = vi.fn();
    const delta: DeltaSlice = {
      bytes_b64: btoa("delta"),
      head_seq: 2,
    };

    const dataDisposable = renderer.onData(dataHandler);
    const resizeDisposable = renderer.onResize(resizeHandler);

    expect(terminalInstances[0]?.onData).toHaveBeenCalledWith(dataHandler);
    const xtermResizeHandler = terminalInstances[0]?.onResize.mock.calls[0]?.[0];
    xtermResizeHandler?.({ cols: 120, rows: 40 });

    expect(resizeHandler).toHaveBeenCalledWith(120, 40);
    expect(dataDisposable).toEqual({ dispose: expect.any(Function) });
    expect(resizeDisposable).toEqual({ dispose: expect.any(Function) });
    renderer.resize(100, 40);
    expect(terminalInstances[0]?.resize).toHaveBeenCalledWith(100, 40);
    expect(renderer.proposeDimensions()).toEqual({ cols: 100, rows: 30 });

    await renderer.applySnapshot(snapshot());
    await renderer.applyDelta(delta);

    const terminal = terminalInstances[0];
    expect(terminal?.reset).toHaveBeenCalledOnce();
    expect(terminal?.resize).toHaveBeenCalledWith(80, 24);
    expect(terminal?.write).toHaveBeenLastCalledWith(
      new TextEncoder().encode("delta"),
      expect.any(Function),
    );
  });

  test("falls back when WebGL addon construction fails", () => {
    webglOptions.throwOnCreate = true;

    const renderer = new Renderer();
    renderer.mount({} as HTMLElement);

    expect(terminalInstances[0]?.open).toHaveBeenCalledOnce();
    expect(terminalInstances[0]?.loadAddon).toHaveBeenCalledTimes(2);
    expect(webglAddonInstances).toEqual([]);
  });

  test("disposes a WebGL addon when loading it fails", () => {
    webglOptions.throwOnLoadAddon = true;

    const renderer = new Renderer();
    renderer.mount({} as HTMLElement);

    expect(webglAddonInstances).toHaveLength(1);
    expect(webglAddonInstances[0]?.dispose).toHaveBeenCalledTimes(1);
    expect(terminalInstances[0]?.loadAddon).toHaveBeenCalledTimes(3);
  });
});
