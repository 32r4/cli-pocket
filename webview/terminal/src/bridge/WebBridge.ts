import type {
  ClientBridge,
  ConnectConfig,
  CreateTerminalParams,
} from "./ClientBridge";
import type { ClientEvent } from "@/types/events";
import type { TerminalId } from "@/types/frame";

type CurrentSendInput = (data: Uint8Array) => Promise<void>;
type FutureSendInput = (
  terminalId: TerminalId,
  data: Uint8Array,
) => Promise<void>;
type CurrentResize = (cols: number, rows: number) => Promise<void>;
type FutureResize = (
  terminalId: TerminalId,
  cols: number,
  rows: number,
) => Promise<void>;
type CurrentKill = () => Promise<void>;
type FutureKill = (terminalId: TerminalId, signal: string) => Promise<void>;

export interface WasmClient {
  connect(configJson: string): Promise<void>;
  create_terminal(paramsJson: string): Promise<void>;
  send_input: CurrentSendInput | FutureSendInput;
  resize: CurrentResize | FutureResize;
  kill: CurrentKill | FutureKill;
  next_event(): Promise<unknown>;
  export_identity(): Promise<unknown>;
  import_identity(bytes: Uint8Array): Promise<void>;
  close?: () => Promise<void>;
}

interface WasmModule {
  default?: () => Promise<unknown> | unknown;
  CliPocketClient?: new () => WasmClient;
}

interface TerminalCreateJson {
  cols: number;
  rows: number;
  cwd: string | null;
  cmd: string[];
  env: Array<[string, string]>;
  scrollback_bytes: number | null;
}

export class WebBridge implements ClientBridge {
  private closed = false;

  constructor(private readonly client: WasmClient) {}

  static async create(client?: WasmClient): Promise<WebBridge> {
    if (client !== undefined) {
      return new WebBridge(client);
    }

    const module = await import("cli-pocket-client-core-wasm");
    if (!isWasmModule(module) || module.CliPocketClient === undefined) {
      throw new Error("cli-pocket-client-core-wasm did not export CliPocketClient");
    }

    if (module.default !== undefined) {
      await module.default();
    }

    return new WebBridge(new module.CliPocketClient());
  }

  async connect(config: ConnectConfig): Promise<void> {
    await this.client.connect(
      JSON.stringify({
        endpoint_url: config.endpointUrl,
        server_public_hex: config.serverPublicHex,
        resume_token_hex: config.resumeTokenHex ?? null,
      }),
    );
  }

  events(): AsyncIterable<ClientEvent> {
    return {
      [Symbol.asyncIterator]: () => ({
        next: () => this.nextEvent(),
      }),
    };
  }

  async createTerminal(params: CreateTerminalParams): Promise<void> {
    await this.client.create_terminal(
      JSON.stringify(toTerminalCreateJson(params)),
    );
  }

  async sendInput(terminalId: TerminalId, bytes: Uint8Array): Promise<void> {
    const sendInput = this.client.send_input;
    if (sendInput.length >= 2) {
      await (sendInput as FutureSendInput)(terminalId, bytes);
      return;
    }

    // Plan F wasm targets the active terminal.
    await (sendInput as CurrentSendInput)(bytes);
  }

  async resize(
    terminalId: TerminalId,
    cols: number,
    rows: number,
  ): Promise<void> {
    const resize = this.client.resize;
    if (resize.length >= 3) {
      await (resize as FutureResize)(terminalId, cols, rows);
      return;
    }

    await (resize as CurrentResize)(cols, rows);
  }

  async kill(terminalId: TerminalId, signal: string): Promise<void> {
    const kill = this.client.kill;
    if (kill.length >= 2) {
      await (kill as FutureKill)(terminalId, signal);
      return;
    }

    await (kill as CurrentKill)();
  }

  async exportIdentity(): Promise<Uint8Array> {
    return normalizeIdentity(await this.client.export_identity());
  }

  async importIdentity(blob: Uint8Array): Promise<void> {
    await this.client.import_identity(blob);
  }

  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    await this.client.close?.();
  }

  private async nextEvent(): Promise<IteratorResult<ClientEvent>> {
    if (this.closed) {
      return { value: undefined, done: true };
    }

    const raw = await this.client.next_event();
    if (this.closed || raw === null || raw === undefined) {
      return { value: undefined, done: true };
    }

    if (!isClientEvent(raw)) {
      throw new Error("wasm next_event returned invalid ClientEvent");
    }

    return { value: raw, done: false };
  }
}

function toTerminalCreateJson(params: CreateTerminalParams): TerminalCreateJson {
  return {
    cols: params.cols,
    rows: params.rows,
    cwd: params.cwd ?? null,
    cmd: params.cmd ?? (params.shell === undefined ? [] : [params.shell]),
    env: Object.entries(params.env ?? {}),
    scrollback_bytes: params.scrollbackBytes ?? null,
  };
}

function normalizeIdentity(raw: unknown): Uint8Array {
  if (raw instanceof Uint8Array) {
    return raw;
  }
  if (Array.isArray(raw) && raw.every(isByte)) {
    return new Uint8Array(raw);
  }
  if (typeof raw === "string") {
    return new TextEncoder().encode(raw);
  }
  throw new Error("wasm export_identity returned invalid identity bytes");
}

function isWasmModule(value: unknown): value is WasmModule {
  if (!isRecord(value)) {
    return false;
  }

  const init = value["default"];
  const client = value["CliPocketClient"];
  return (
    (init === undefined || typeof init === "function") &&
    (client === undefined || typeof client === "function")
  );
}

function isClientEvent(value: unknown): value is ClientEvent {
  if (!isRecord(value) || typeof value["kind"] !== "string") {
    return false;
  }

  switch (value["kind"]) {
    case "Connecting":
      return true;
    case "Connected":
      return typeof value["session_id"] === "string";
    case "Disconnected":
      return (
        typeof value["will_retry"] === "boolean" &&
        typeof value["reason"] === "string"
      );
    case "TerminalCreated":
      return isTerminalInfo(value["info"]);
    case "TerminalOutput":
      return (
        typeof value["terminal_id"] === "string" &&
        isNumber(value["stream_seq"]) &&
        typeof value["bytes_b64"] === "string"
      );
    case "TerminalExited":
      return (
        typeof value["terminal_id"] === "string" && isExitInfo(value["info"])
      );
    case "Error":
      return typeof value["message"] === "string";
    default:
      return false;
  }
}

function isTerminalInfo(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value["terminal"] === "string" &&
    isNumber(value["cols"]) &&
    isNumber(value["rows"]) &&
    isNumber(value["created_at_unix_ms"]) &&
    (value["label"] === null || typeof value["label"] === "string") &&
    isNumber(value["attached_clients"])
  );
}

function isExitInfo(value: unknown): boolean {
  return (
    isRecord(value) &&
    (value["code"] === null || isNumber(value["code"])) &&
    (value["signal"] === null || isNumber(value["signal"])) &&
    isNumber(value["at_unix_ms"])
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isByte(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= 255
  );
}
