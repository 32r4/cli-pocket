import type { IDisposable } from "@xterm/xterm";

import type { ClientBridge, CreateTerminalParams } from "@/bridge/ClientBridge";
import { Renderer } from "@/render/Renderer";
import type { ClientEvent } from "@/types/events";
import type { TerminalId } from "@/types/frame";
import { StatusBar, type ConnectionState } from "./StatusBar";
import { VirtualKeyBar } from "./VirtualKeyBar";

type ClientKind = "tauri" | "web";

interface AppState {
  connection: ConnectionState;
  terminalId: TerminalId | null;
  error: string | null;
}

export class App {
  private readonly renderer = new Renderer();
  private readonly statusBar: StatusBar;
  private readonly terminalHost = document.createElement("main");
  private readonly disposables: IDisposable[] = [];
  private state: AppState = {
    connection: "idle",
    terminalId: null,
    error: null,
  };
  private disposed = false;
  private eventLoop: Promise<void> | null = null;
  private terminalCreatePending = false;

  constructor(
    private readonly host: HTMLElement,
    private readonly bridge: ClientBridge,
    private readonly clientKind: ClientKind,
  ) {
    this.statusBar = new StatusBar(this.state);
  }

  async start(): Promise<void> {
    this.host.replaceChildren();
    this.host.className = "app-shell";
    this.terminalHost.className = "terminal-screen";
    this.host.append(this.terminalHost, this.statusBar.element);

    if (this.clientKind === "tauri") {
      const virtualKeyBar = new VirtualKeyBar((bytes) => {
        void this.sendBytes(bytes);
      });
      this.host.append(virtualKeyBar.element);
    }

    this.renderer.mount(this.terminalHost);
    this.disposables.push(
      this.renderer.onData((data) => {
        void this.sendBytes(new TextEncoder().encode(data));
      }),
      this.renderer.onResize((cols, rows) => {
        void this.resize(cols, rows);
      }),
    );

    this.eventLoop = this.consumeEvents();
  }

  async dispose(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.disposed = true;

    for (const disposable of this.disposables.splice(0)) {
      disposable.dispose();
    }

    this.renderer.dispose();
    await this.bridge.close();
    await this.eventLoop?.catch(() => undefined);
  }

  private createTerminalParams(): CreateTerminalParams {
    return this.renderer.proposeDimensions() ?? { cols: 80, rows: 24 };
  }

  private async consumeEvents(): Promise<void> {
    try {
      for await (const event of this.bridge.events()) {
        if (this.disposed) {
          return;
        }
        await this.handleEvent(event);
      }
    } catch (error) {
      if (!this.disposed) {
        this.setState({ error: errorMessage(error) });
      }
    }
  }

  private async handleEvent(event: ClientEvent): Promise<void> {
    switch (event.kind) {
      case "Connecting":
        this.setState({ connection: "connecting", error: null });
        return;
      case "Connected":
        this.setState({ connection: "connected", error: null });
        await this.requestTerminal();
        return;
      case "Disconnected":
        this.setState({
          connection: "disconnected",
          error: event.will_retry ? event.reason : null,
        });
        return;
      case "TerminalCreated":
        this.setState({
          terminalId: event.info.terminal,
          error: null,
        });
        this.renderer.resize(event.info.cols, event.info.rows);
        return;
      case "TerminalOutput":
        if (event.terminal_id === this.state.terminalId) {
          await this.renderer.applyDelta({
            bytes_b64: event.bytes_b64,
            head_seq: event.stream_seq,
          });
        }
        return;
      case "TerminalExited":
        if (event.terminal_id === this.state.terminalId) {
          this.setState({ terminalId: null });
        }
        return;
      case "Error":
        this.setState({ error: event.message });
        return;
    }
  }

  private async sendBytes(bytes: Uint8Array): Promise<void> {
    const terminalId = this.state.terminalId;
    if (terminalId === null) {
      return;
    }

    try {
      await this.bridge.sendInput(terminalId, bytes);
    } catch (error) {
      this.setState({ error: errorMessage(error) });
    }
  }

  private async resize(cols: number, rows: number): Promise<void> {
    const terminalId = this.state.terminalId;
    if (terminalId === null) {
      return;
    }

    try {
      await this.bridge.resize(terminalId, cols, rows);
    } catch (error) {
      this.setState({ error: errorMessage(error) });
    }
  }

  private async requestTerminal(): Promise<void> {
    if (
      this.disposed ||
      this.state.terminalId !== null ||
      this.terminalCreatePending
    ) {
      return;
    }

    this.terminalCreatePending = true;
    try {
      await this.bridge.createTerminal(this.createTerminalParams());
    } catch (error) {
      if (!this.disposed) {
        this.setState({ error: errorMessage(error) });
      }
    } finally {
      this.terminalCreatePending = false;
    }
  }

  private setState(nextState: Partial<AppState>): void {
    this.state = { ...this.state, ...nextState };
    this.statusBar.update(this.state);
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
