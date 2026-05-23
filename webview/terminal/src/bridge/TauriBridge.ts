import type {
  ClientBridge,
  ConnectConfig,
  CreateTerminalParams,
} from "./ClientBridge";
import type { ClientEvent } from "@/types/events";
import type { TerminalId } from "@/types/frame";
import type { InvokeArgs } from "@tauri-apps/api/core";

const EVENT_CHANNEL = "cli_pocket:event";

const COMMANDS = {
  connect: "cli_pocket_connect",
  createTerminal: "cli_pocket_create_terminal",
  sendInput: "cli_pocket_send_input",
  resize: "cli_pocket_resize",
  kill: "cli_pocket_kill",
  exportIdentity: "cli_pocket_export_identity",
  importIdentity: "cli_pocket_import_identity",
  close: "cli_pocket_close",
} as const;

type TauriCore = typeof import("@tauri-apps/api/core");
type TauriEvent = typeof import("@tauri-apps/api/event");
type Unlisten = () => void;
type EventWaiter = {
  resolve: (result: IteratorResult<ClientEvent>) => void;
  reject: (reason: unknown) => void;
};

export class TauriBridge implements ClientBridge {
  private queuedEvents: ClientEvent[] = [];
  private pendingWaiters: EventWaiter[] = [];
  private listenSetupPromise: Promise<void> | null = null;
  private listenError: unknown = null;
  private unlisten: Unlisten | null = null;
  private closed = false;

  async connect(config: ConnectConfig): Promise<void> {
    await this.invokeVoid(COMMANDS.connect, { config });
  }

  events(): AsyncIterable<ClientEvent> {
    return {
      [Symbol.asyncIterator]: () => ({
        next: () => this.nextEvent(),
      }),
    };
  }

  async createTerminal(params: CreateTerminalParams): Promise<void> {
    await this.invokeVoid(COMMANDS.createTerminal, { params });
  }

  async sendInput(terminalId: TerminalId, bytes: Uint8Array): Promise<void> {
    await this.invokeVoid(COMMANDS.sendInput, {
      terminalId,
      bytes: Array.from(bytes),
    });
  }

  async resize(
    terminalId: TerminalId,
    cols: number,
    rows: number,
  ): Promise<void> {
    await this.invokeVoid(COMMANDS.resize, { terminalId, cols, rows });
  }

  async kill(terminalId: TerminalId, signal: string): Promise<void> {
    await this.invokeVoid(COMMANDS.kill, { terminalId, signal });
  }

  async exportIdentity(): Promise<Uint8Array> {
    const blob = await this.invoke<unknown>(COMMANDS.exportIdentity);
    if (blob instanceof Uint8Array) {
      return blob;
    }
    if (Array.isArray(blob) && blob.every((byte) => this.isByte(byte))) {
      return new Uint8Array(blob);
    }
    throw new Error("Tauri export_identity returned invalid identity bytes");
  }

  async importIdentity(blob: Uint8Array): Promise<void> {
    await this.invokeVoid(COMMANDS.importIdentity, {
      blob: Array.from(blob),
    });
  }

  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.queuedEvents = [];

    const pending = this.pendingWaiters.splice(0);
    for (const { resolve } of pending) {
      resolve({ value: undefined, done: true });
    }

    await this.listenSetupPromise?.catch(() => undefined);
    this.unlisten?.();
    this.unlisten = null;
    await this.invokeVoid(COMMANDS.close);
  }

  private async invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
    const { invoke } = await this.loadCore();
    return invoke<T>(command, args);
  }

  private async invokeVoid(command: string, args?: InvokeArgs): Promise<void> {
    await this.invoke<unknown>(command, args);
  }

  private async ensureListening(): Promise<void> {
    if (this.listenSetupPromise !== null) {
      return;
    }

    this.listenSetupPromise = this.setupListener();
    await this.listenSetupPromise;
  }

  private async setupListener(): Promise<void> {
    try {
      const { listen } = await this.loadEvent();
      const unlisten = await listen<ClientEvent>(EVENT_CHANNEL, (event) => {
        this.enqueueEvent(event.payload);
      });
      if (this.closed) {
        unlisten();
        return;
      }
      this.unlisten = unlisten;
    } catch (error) {
      this.listenError = error;
      const pending = this.pendingWaiters.splice(0);
      for (const { reject } of pending) {
        reject(error);
      }
      throw error;
    }
  }

  private async nextEvent(): Promise<IteratorResult<ClientEvent>> {
    if (!this.closed) {
      await this.ensureListening();
    }

    if (this.listenError !== null) {
      throw this.listenError;
    }

    if (this.queuedEvents.length > 0) {
      const event = this.queuedEvents.shift();
      if (event !== undefined) {
        return { value: event, done: false };
      }
    }

    if (this.closed) {
      return { value: undefined, done: true };
    }

    return new Promise((resolve, reject) => {
      this.pendingWaiters.push({ resolve, reject });
    });
  }

  private enqueueEvent(event: ClientEvent): void {
    if (this.closed) {
      return;
    }

    const waiter = this.pendingWaiters.shift();
    if (waiter !== undefined) {
      waiter.resolve({ value: event, done: false });
      return;
    }

    this.queuedEvents.push(event);
  }

  private isByte(value: unknown): value is number {
    return (
      typeof value === "number" &&
      Number.isInteger(value) &&
      value >= 0 &&
      value <= 255
    );
  }

  private async loadCore(): Promise<TauriCore> {
    return import("@tauri-apps/api/core");
  }

  private async loadEvent(): Promise<TauriEvent> {
    return import("@tauri-apps/api/event");
  }
}
