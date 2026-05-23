import type { ExitInfo, StreamSeq, TerminalId, TerminalInfo } from "./frame";

export type ClientEvent =
  | { kind: "Connecting" }
  | { kind: "Connected"; session_id: string }
  | { kind: "Disconnected"; will_retry: boolean; reason: string }
  | { kind: "TerminalCreated"; info: TerminalInfo }
  | {
      kind: "TerminalOutput";
      terminal_id: TerminalId;
      stream_seq: StreamSeq;
      bytes_b64: string;
    }
  | { kind: "TerminalExited"; terminal_id: TerminalId; info: ExitInfo }
  | { kind: "Error"; message: string };
