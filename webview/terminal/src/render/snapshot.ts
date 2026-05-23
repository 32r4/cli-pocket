import { base64ToBytes, writeBytes, type TerminalWriter } from "@/render/delta";
import type {
  AnchorState,
  CharsetState,
  Color,
  MouseMode,
  SgrAttrs,
  Snapshot,
} from "@/types/frame";

interface SnapshotTerminal extends TerminalWriter {
  reset(): void;
  resize(cols: number, rows: number): void;
}

const ESC = "\x1b";
const BEL = "\x07";
const SI = "\x0f";
const SO = "\x0e";

export async function applySnapshot(
  term: SnapshotTerminal,
  snap: Snapshot,
): Promise<void> {
  term.reset();
  term.resize(snap.cols, snap.rows);

  const anchorBytes = encodeAnchorState(snap.anchor_state);
  if (anchorBytes.length > 0) {
    await writeBytes(term, anchorBytes);
  }

  await writeBytes(term, base64ToBytes(snap.bytes_b64));
}

function encodeAnchorState(anchor: AnchorState): Uint8Array {
  return new TextEncoder().encode(
    [
      encodeTitle(anchor.title),
      anchor.modes.deccmm_cursor_keys ? encodeMode("1", true) : "",
      anchor.modes.autowrap ? "" : encodeMode("7", false),
      anchor.modes.alt_screen ? encodeMode("1049", true) : "",
      anchor.modes.bracketed_paste ? encodeMode("2004", true) : "",
      encodeMouseMode(anchor.modes.mouse_reporting),
      anchor.modes.origin_mode ? encodeMode("6", true) : "",
      encodeSgr(anchor.sgr),
      encodeCharset(anchor.charset),
      encodeCursor(anchor.cursor),
    ].join(""),
  );
}

function encodeTitle(title: string | null): string {
  return title === null ? "" : `${ESC}]0;${sanitizeTitle(title)}${BEL}`;
}

function sanitizeTitle(title: string): string {
  return Array.from(title)
    .filter((char) => {
      const codePoint = char.codePointAt(0);
      return (
        codePoint !== undefined &&
        !(
          (codePoint >= 0x00 && codePoint <= 0x1f) ||
          (codePoint >= 0x7f && codePoint <= 0x9f)
        )
      );
    })
    .join("");
}

function encodeMode(mode: string, enabled: boolean): string {
  return `${ESC}[?${mode}${enabled ? "h" : "l"}`;
}

function encodeMouseMode(mouseMode: MouseMode): string {
  switch (mouseMode) {
    case "Off":
      return "";
    case "X10":
      return encodeMode("9", true);
    case "Normal":
      return encodeMode("1000", true);
    case "ButtonEvent":
      return encodeMode("1002", true);
    case "AnyEvent":
      return encodeMode("1003", true);
  }
}

function encodeSgr(sgr: SgrAttrs): string {
  const codes = [
    sgr.bold ? "1" : null,
    sgr.faint ? "2" : null,
    sgr.italic ? "3" : null,
    sgr.underline ? "4" : null,
    sgr.blink ? "5" : null,
    sgr.reverse ? "7" : null,
    sgr.strikethrough ? "9" : null,
    ...encodeColor(sgr.fg, false),
    ...encodeColor(sgr.bg, true),
  ].filter((code): code is string => code !== null);

  return `${ESC}[0m${codes.length > 0 ? `${ESC}[${codes.join(";")}m` : ""}`;
}

function encodeColor(color: Color | null, background: boolean): string[] {
  if (color === null) {
    return [];
  }

  if ("Palette" in color) {
    if (color.Palette >= 0 && color.Palette <= 7) {
      return [String(color.Palette + (background ? 40 : 30))];
    }

    if (color.Palette >= 8 && color.Palette <= 15) {
      return [String(color.Palette - 8 + (background ? 100 : 90))];
    }

    return [background ? "48" : "38", "5", String(color.Palette)];
  }

  if ("Indexed" in color) {
    return [background ? "48" : "38", "5", String(color.Indexed)];
  }

  return [
    background ? "48" : "38",
    "2",
    String(color.Rgb[0]),
    String(color.Rgb[1]),
    String(color.Rgb[2]),
  ];
}

function encodeCharset(charset: CharsetState): string {
  const designators = ["(", ")", "*", "+"];
  const designations = charset.g
    .map((finalByte, index) => `${ESC}${designators[index]}${String.fromCharCode(finalByte)}`)
    .join("");

  // xterm exposes SI/SO for GL G0/G1. Restoring arbitrary GL G2/G3 and GR
  // safely would require invasive terminal state changes, so leave them intact.
  const glShift = charset.gl === 0 ? SI : charset.gl === 1 ? SO : "";

  return `${designations}${glShift}`;
}

function encodeCursor(cursor: [number, number]): string {
  const [col, row] = cursor;
  return `${ESC}[${row + 1};${col + 1}H`;
}
