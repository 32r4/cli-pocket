import { FitAddon, type ITerminalDimensions } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal, type IDisposable } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { applyDelta as applyDeltaToTerminal } from "@/render/delta";
import { applySnapshot as applySnapshotToTerminal } from "@/render/snapshot";
import type { DeltaSlice, Snapshot } from "@/types/frame";

export type ResizeHandler = (cols: number, rows: number) => void;

export class Renderer {
  readonly terminal: Terminal;

  private readonly fitAddon: FitAddon;
  private mountedContainer: HTMLElement | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private webglAddon: WebglAddon | null = null;
  private webglContextLossDisposable: IDisposable | null = null;
  private disposed = false;

  constructor() {
    this.terminal = new Terminal({
      allowProposedApi: true,
    });
    this.fitAddon = new FitAddon();

    this.terminal.loadAddon(this.fitAddon);

    const unicodeAddon = new Unicode11Addon();
    this.terminal.loadAddon(unicodeAddon);
    this.terminal.unicode.activeVersion = "11";
  }

  mount(container: HTMLElement): void {
    if (this.disposed) {
      throw new Error("Renderer has been disposed");
    }

    if (this.mountedContainer !== null && this.mountedContainer !== container) {
      throw new Error("Renderer is already mounted to a different container");
    }

    if (this.mountedContainer === container) {
      this.fitAddon.fit();
      return;
    }

    this.disconnectResizeObserver();
    this.terminal.open(container);
    this.loadWebglAddon();
    this.fitAddon.fit();
    this.terminal.focus?.();
    this.mountedContainer = container;

    this.resizeObserver = new ResizeObserver(() => {
      this.fitAddon.fit();
    });
    this.resizeObserver.observe(container);
  }

  onData(handler: (data: string) => void): IDisposable {
    return this.terminal.onData(handler);
  }

  onResize(handler: ResizeHandler): IDisposable {
    return this.terminal.onResize(({ cols, rows }) => {
      handler(cols, rows);
    });
  }

  resize(cols: number, rows: number): void {
    this.terminal.resize(cols, rows);
  }

  proposeDimensions(): ITerminalDimensions | undefined {
    return this.fitAddon.proposeDimensions();
  }

  async applySnapshot(snap: Snapshot): Promise<void> {
    await applySnapshotToTerminal(this.terminal, snap);
  }

  async applyDelta(delta: DeltaSlice): Promise<void> {
    await applyDeltaToTerminal(this.terminal, delta);
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }

    this.disconnectResizeObserver();
    this.disposeWebglAddon();
    this.terminal.dispose();
    this.mountedContainer = null;
    this.disposed = true;
  }

  private disconnectResizeObserver(): void {
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
  }

  private loadWebglAddon(): void {
    if (this.webglAddon !== null) {
      return;
    }

    try {
      const webglAddon = new WebglAddon();
      this.webglContextLossDisposable = webglAddon.onContextLoss(() => {
        this.disposeWebglAddon();
      });
      try {
        this.terminal.loadAddon(webglAddon);
        this.webglAddon = webglAddon;
      } catch {
        webglAddon.dispose();
        throw new Error("failed to load webgl addon");
      }
    } catch {
      this.disposeWebglAddon();
    }
  }

  private disposeWebglAddon(): void {
    this.webglContextLossDisposable?.dispose();
    this.webglContextLossDisposable = null;
    this.webglAddon?.dispose();
    this.webglAddon = null;
  }
}
