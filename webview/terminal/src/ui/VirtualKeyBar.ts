import { virtualKeyToBytes, type VirtualKey } from "@/input/keymap";

export type VirtualKeyHandler = (bytes: Uint8Array) => void;

const VIRTUAL_KEYS: readonly VirtualKey[] = [
  "Esc",
  "Tab",
  "Ctrl+C",
  "Ctrl+D",
  "Ctrl+L",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
];

export class VirtualKeyBar {
  readonly element: HTMLElement;

  constructor(onKey: VirtualKeyHandler) {
    this.element = document.createElement("nav");
    this.element.className = "virtual-key-bar";

    for (const key of VIRTUAL_KEYS) {
      const button = document.createElement("button");
      button.className = "virtual-key-bar__key";
      button.type = "button";
      button.textContent = key;
      button.dataset["virtualKey"] = key;
      button.addEventListener("click", () => {
        onKey(virtualKeyToBytes(key));
      });
      this.element.append(button);
    }
  }
}
