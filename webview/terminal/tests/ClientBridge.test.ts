import { describe, expectTypeOf, test } from "vitest";

import type {
  ClientBridge,
  ConnectConfig,
  CreateTerminalParams,
} from "@/bridge/ClientBridge";
import { CLIENT_KIND } from "@/bridge/ClientBridge";
import type { ClientEvent } from "@/types/events";

describe("ClientBridge contract", () => {
  test("uses frontend config names", () => {
    expectTypeOf<ConnectConfig>().toEqualTypeOf<{
      endpointUrl: string;
      serverPublicHex: string;
      resumeTokenHex?: string;
    }>();
  });

  test("create terminal params are camelCase and proto-aligned", () => {
    expectTypeOf<CreateTerminalParams>().toEqualTypeOf<{
      cols: number;
      rows: number;
      cwd?: string;
      cmd?: string[];
      shell?: string;
      env?: Record<string, string>;
      scrollbackBytes?: number;
    }>();
  });

  test("bridge exposes client event stream and client kind", () => {
    expectTypeOf<ClientBridge["events"]>().returns.toEqualTypeOf<
      AsyncIterable<ClientEvent>
    >();
    expectTypeOf<ClientBridge["createTerminal"]>().returns.toEqualTypeOf<
      Promise<void>
    >();
    expectTypeOf<typeof CLIENT_KIND>().toEqualTypeOf<"tauri" | "web">();
  });
});
