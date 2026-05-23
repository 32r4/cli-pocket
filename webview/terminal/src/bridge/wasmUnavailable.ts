import type { WasmClient } from "./WebBridge";

const UNAVAILABLE_MESSAGE =
  "cli-pocket-client-core-wasm package has not been built; run just build-wasm";

export default async function init(): Promise<never> {
  throw new Error(UNAVAILABLE_MESSAGE);
}

export class CliPocketClient implements WasmClient {
  async connect(_configJson: string): Promise<never> {
    throwUnavailable();
  }

  async create_terminal(_paramsJson: string): Promise<never> {
    throwUnavailable();
  }

  async send_input(_dataOrTerminalId: Uint8Array | string): Promise<never> {
    throwUnavailable();
  }

  async resize(_colsOrTerminalId: number | string): Promise<never> {
    throwUnavailable();
  }

  async kill(_terminalId?: string): Promise<never> {
    throwUnavailable();
  }

  async next_event(): Promise<never> {
    throwUnavailable();
  }

  async export_identity(): Promise<never> {
    throwUnavailable();
  }

  async import_identity(_blob: string): Promise<never> {
    throwUnavailable();
  }

  async close(): Promise<never> {
    throwUnavailable();
  }
}

function throwUnavailable(): never {
  throw new Error(UNAVAILABLE_MESSAGE);
}
