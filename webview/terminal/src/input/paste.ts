const encoder = new TextEncoder();

export async function getClipboardText(): Promise<string> {
  if (typeof navigator === "undefined" || typeof navigator.clipboard?.readText !== "function") {
    return "";
  }

  try {
    return await navigator.clipboard.readText();
  } catch {
    return "";
  }
}

export function wrapBracketedPaste(text: string): Uint8Array {
  return encoder.encode(`\u001b[200~${text}\u001b[201~`);
}
