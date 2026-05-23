export type VirtualKey =
  | "Esc"
  | "Tab"
  | "ArrowUp"
  | "ArrowDown"
  | "ArrowLeft"
  | "ArrowRight"
  | "Home"
  | "End"
  | "PageUp"
  | "PageDown"
  | "Ctrl+C"
  | "Ctrl+D"
  | "Ctrl+Z"
  | "Ctrl+L"
  | "Ctrl+R"
  | "Ctrl+U"
  | "Ctrl+W"
  | "Pipe"
  | "Tilde";

const virtualKeyBytes: Readonly<Record<VirtualKey, readonly number[]>> = {
  Esc: [0x1b],
  Tab: [0x09],
  ArrowUp: [0x1b, 0x5b, 0x41],
  ArrowDown: [0x1b, 0x5b, 0x42],
  ArrowRight: [0x1b, 0x5b, 0x43],
  ArrowLeft: [0x1b, 0x5b, 0x44],
  Home: [0x1b, 0x5b, 0x48],
  End: [0x1b, 0x5b, 0x46],
  PageUp: [0x1b, 0x5b, 0x35, 0x7e],
  PageDown: [0x1b, 0x5b, 0x36, 0x7e],
  "Ctrl+C": [0x03],
  "Ctrl+D": [0x04],
  "Ctrl+Z": [0x1a],
  "Ctrl+L": [0x0c],
  "Ctrl+R": [0x12],
  "Ctrl+U": [0x15],
  "Ctrl+W": [0x17],
  Pipe: [0x7c],
  Tilde: [0x7e],
};

export function virtualKeyToBytes(key: VirtualKey): Uint8Array {
  return new Uint8Array(virtualKeyBytes[key]);
}
