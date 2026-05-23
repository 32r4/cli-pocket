export type Base64String = string;

// UUID newtypes are strings at the webview JSON boundary. Rust postcard
// encoding uses the internal newtype/UUID byte shape instead.
export type UuidString = string;
export type TerminalId = UuidString;
export type SessionId = UuidString;
export type HostId = UuidString;
export type ClientId = UuidString;

export type StreamId = number;
export type StreamSeq = number;
export type UnixMillis = number;

export interface TerminalCreateParams {
  cols: number;
  rows: number;
  cwd: string | null;
  cmd: string[];
  env: Array<[string, string]>;
  scrollback_bytes: number | null;
}

export interface TerminalInfo {
  terminal: TerminalId;
  cols: number;
  rows: number;
  created_at_unix_ms: UnixMillis;
  label: string | null;
  attached_clients: number;
}

export interface ExitInfo {
  code: number | null;
  signal: number | null;
  at_unix_ms: UnixMillis;
}

export interface Snapshot {
  cols: number;
  rows: number;
  anchor_state: AnchorState;
  bytes_b64: Base64String;
  head_seq: StreamSeq;
}

export interface DeltaSlice {
  bytes_b64: Base64String;
  head_seq: StreamSeq;
}

export interface AnchorState {
  cursor: [number, number];
  sgr: SgrAttrs;
  modes: TerminalModes;
  charset: CharsetState;
  title: string | null;
}

export interface SgrAttrs {
  fg: Color | null;
  bg: Color | null;
  bold: boolean;
  faint: boolean;
  italic: boolean;
  underline: boolean;
  blink: boolean;
  reverse: boolean;
  strikethrough: boolean;
}

export type Color =
  | { Palette: number }
  | { Indexed: number }
  | { Rgb: [number, number, number] };

export interface TerminalModes {
  deccmm_cursor_keys: boolean;
  autowrap: boolean;
  alt_screen: boolean;
  bracketed_paste: boolean;
  mouse_reporting: MouseMode;
  origin_mode: boolean;
}

export type MouseMode = "Off" | "X10" | "Normal" | "ButtonEvent" | "AnyEvent";

export interface CharsetState {
  g: [number, number, number, number];
  gl: number;
  gr: number;
}
