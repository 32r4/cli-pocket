declare module "cli-pocket-client-core-wasm" {
  import type { WasmClient } from "@/bridge/WebBridge";

  export default function init(): Promise<unknown> | unknown;

  export class CliPocketClient implements WasmClient {
    connect(configJson: string): Promise<void>;
    create_terminal(paramsJson: string): Promise<void>;
    send_input:
      | ((data: Uint8Array) => Promise<void>)
      | ((terminalId: string, data: Uint8Array) => Promise<void>);
    resize:
      | ((cols: number, rows: number) => Promise<void>)
      | ((terminalId: string, cols: number, rows: number) => Promise<void>);
    kill:
      | (() => Promise<void>)
      | ((terminalId: string, signal: string) => Promise<void>);
    next_event(): Promise<unknown>;
    export_identity(): string;
    import_identity(blob: string): Promise<void>;
    close?: () => Promise<void>;
  }
}
