import type { ClientEvent } from "@/types/events";
import type { TerminalId } from "@/types/frame";

declare const __CLIENT_KIND__: "tauri" | "web" | undefined;

export const CLIENT_KIND: "tauri" | "web" =
  typeof __CLIENT_KIND__ === "string" ? __CLIENT_KIND__ : "web";

export interface ConnectConfig {
  endpointUrl: string;
  serverPublicHex: string;
  resumeTokenHex?: string;
}

export interface CreateTerminalParams {
  cols: number;
  rows: number;
  cwd?: string;
  cmd?: string[];
  shell?: string;
  env?: Record<string, string>;
  scrollbackBytes?: number;
}

export interface ClientBridge {
  connect(config: ConnectConfig): Promise<void>;
  events(): AsyncIterable<ClientEvent>;
  createTerminal(params: CreateTerminalParams): Promise<TerminalId>;
  sendInput(terminalId: TerminalId, bytes: Uint8Array): Promise<void>;
  resize(terminalId: TerminalId, cols: number, rows: number): Promise<void>;
  kill(terminalId: TerminalId, signal: string): Promise<void>;
  exportIdentity(): Promise<Uint8Array>;
  importIdentity(blob: Uint8Array): Promise<void>;
  close(): Promise<void>;
}
