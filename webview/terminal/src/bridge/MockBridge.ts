import type {
  ClientBridge,
  ConnectConfig,
  CreateTerminalParams,
} from "./ClientBridge";
import type { ClientEvent } from "@/types/events";
import type { StreamSeq, TerminalId } from "@/types/frame";

const MOCK_SESSION_ID = "mock-session";
const MOCK_TERMINAL_ID = "mock-terminal";
const MOCK_IDENTITY = "cli-pocket-mock-identity";
const BASE64_CHUNK_SIZE = 0x8000;

type EventWaiter = (result: IteratorResult<ClientEvent>) => void;

export class MockBridge implements ClientBridge {
  private readonly queuedEvents: ClientEvent[] = [];
  private readonly pendingWaiters: EventWaiter[] = [];
  private readonly encoder = new TextEncoder();
  private activeTerminalId: TerminalId | null = null;
  private streamSeq: StreamSeq = 0;
  private closed = false;
  private identity = this.encoder.encode(MOCK_IDENTITY);

  async connect(_config: ConnectConfig): Promise<void> {
    this.enqueueEvent({ kind: "Connecting" });
    this.enqueueEvent({ kind: "Connected", session_id: MOCK_SESSION_ID });
  }

  events(): AsyncIterable<ClientEvent> {
    return {
      [Symbol.asyncIterator]: () => ({
        next: () => this.nextEvent(),
      }),
    };
  }

  async createTerminal(params: CreateTerminalParams): Promise<void> {
    this.activeTerminalId = MOCK_TERMINAL_ID;
    this.enqueueEvent({
      kind: "TerminalCreated",
      info: {
        terminal: MOCK_TERMINAL_ID,
        cols: params.cols,
        rows: params.rows,
        created_at_unix_ms: Date.now(),
        label: "Mock terminal",
        attached_clients: 1,
      },
    });
    this.emitOutput("Welcome to cli-pocket mock terminal\r\n$ ");
  }

  async sendInput(terminalId: TerminalId, bytes: Uint8Array): Promise<void> {
    if (terminalId !== this.activeTerminalId) {
      return;
    }

    const input = new TextDecoder().decode(bytes).replace(/\r?\n?$/, "");
    this.emitOutput(`${input}\r\n$ `);
  }

  async resize(
    _terminalId: TerminalId,
    _cols: number,
    _rows: number,
  ): Promise<void> {
    return;
  }

  async kill(terminalId: TerminalId, _signal: string): Promise<void> {
    if (terminalId !== this.activeTerminalId) {
      return;
    }

    this.activeTerminalId = null;
    this.enqueueEvent({
      kind: "TerminalExited",
      terminal_id: terminalId,
      info: {
        code: 0,
        signal: null,
        at_unix_ms: Date.now(),
      },
    });
  }

  async exportIdentity(): Promise<Uint8Array> {
    return new Uint8Array(this.identity);
  }

  async importIdentity(blob: Uint8Array): Promise<void> {
    this.identity = new Uint8Array(blob);
  }

  async close(): Promise<void> {
    if (this.closed) {
      return;
    }

    this.closed = true;
    this.queuedEvents.length = 0;
    const waiters = this.pendingWaiters.splice(0);
    for (const waiter of waiters) {
      waiter({ value: undefined, done: true });
    }
  }

  private nextEvent(): Promise<IteratorResult<ClientEvent>> {
    const event = this.queuedEvents.shift();
    if (event !== undefined) {
      return Promise.resolve({ value: event, done: false });
    }

    if (this.closed) {
      return Promise.resolve({ value: undefined, done: true });
    }

    return new Promise((resolve) => {
      this.pendingWaiters.push(resolve);
    });
  }

  private enqueueEvent(event: ClientEvent): void {
    if (this.closed) {
      return;
    }

    const waiter = this.pendingWaiters.shift();
    if (waiter !== undefined) {
      waiter({ value: event, done: false });
      return;
    }

    this.queuedEvents.push(event);
  }

  private emitOutput(text: string): void {
    if (this.activeTerminalId === null) {
      return;
    }

    this.streamSeq += 1;
    this.enqueueEvent({
      kind: "TerminalOutput",
      terminal_id: this.activeTerminalId,
      stream_seq: this.streamSeq,
      bytes_b64: encodeBase64(this.encoder.encode(text)),
    });
  }
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK_SIZE) {
    const chunk = bytes.subarray(offset, offset + BASE64_CHUNK_SIZE);
    for (const byte of chunk) {
      binary += String.fromCharCode(byte);
    }
  }

  return btoa(binary);
}
