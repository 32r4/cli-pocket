import type { TerminalId } from "@/types/frame";

export type ConnectionState = "idle" | "connecting" | "connected" | "disconnected";

export interface StatusBarState {
  connection: ConnectionState;
  terminalId: TerminalId | null;
  error: string | null;
}

export class StatusBar {
  readonly element: HTMLElement;

  private readonly connectionElement: HTMLElement;
  private readonly terminalElement: HTMLElement;
  private readonly errorElement: HTMLElement;

  constructor(initialState: StatusBarState) {
    this.element = document.createElement("footer");
    this.element.className = "status-bar";

    this.connectionElement = this.createItem("status-bar__item--connection");
    this.terminalElement = this.createItem("status-bar__item--terminal");
    this.errorElement = this.createItem("status-bar__item--error");

    this.element.append(
      this.connectionElement,
      this.terminalElement,
      this.errorElement,
    );
    this.update(initialState);
  }

  update(state: StatusBarState): void {
    this.connectionElement.textContent = state.connection;
    this.terminalElement.textContent = state.terminalId ?? "none";
    this.errorElement.textContent = state.error ?? "";
    this.errorElement.toggleAttribute("hidden", state.error === null);
  }

  private createItem(className: string): HTMLElement {
    const item = document.createElement("span");
    item.className = `status-bar__item ${className}`;
    return item;
  }
}
