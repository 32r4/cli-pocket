import { describe, expect, test } from "vitest";
import { deriveDaemonEndpoints } from "./relayEndpoint";

describe("deriveDaemonEndpoints", () => {
  test("derives pair and session URLs from daemon URL", () => {
    expect(deriveDaemonEndpoints("ws://127.0.0.1:7842")).toEqual({
      pairing_url: "ws://127.0.0.1:7842/pair",
      session_url: "ws://127.0.0.1:7842/session",
    });
  });

  test("replaces an existing path with pair and session paths", () => {
    expect(deriveDaemonEndpoints("wss://host.example/base")).toEqual({
      pairing_url: "wss://host.example/pair",
      session_url: "wss://host.example/session",
    });
  });
});
