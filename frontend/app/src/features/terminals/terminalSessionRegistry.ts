import type { StoreApi } from "zustand/vanilla";
import type {
	SessionActor,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
import type { TerminalController } from "./terminalController";
import { TerminalSessionActor } from "./terminalSessionActor";

interface WorkspaceStoreShape {
	markTerminalConnecting: (terminalId: string) => void;
	markTerminalReady: (info: TerminalSnapshotRecord["info"]) => void;
	markTerminalError: (terminalId: string, message: string) => void;
	updateTerminalSize: (terminalId: string, cols: number, rows: number) => void;
}

interface TerminalSessionRegistryDeps {
	controller: TerminalController;
	workspaceState: StoreApi<WorkspaceStoreShape>;
	session: () => SessionActor | null;
	onInlineError: (message: string | null) => void;
}

export class TerminalSessionRegistry {
	private actors = new Map<string, TerminalSessionActor>();
	private activeTerminalId: string | null = null;

	constructor(private readonly deps: TerminalSessionRegistryDeps) {}

	activateTerminal(terminalId: string, connectionGeneration: number) {
		if (this.activeTerminalId != null && this.activeTerminalId !== terminalId) {
			this.actors.get(this.activeTerminalId)?.detach();
		}
		this.activeTerminalId = terminalId;
		this.actor(terminalId).activateTerminal(connectionGeneration);
	}

	applyOutput(
		terminalId: string,
		seq: number,
		chunk: string,
		connectionGeneration: number,
	) {
		this.actors
			.get(terminalId)
			?.applyOutput(terminalId, seq, chunk, connectionGeneration);
	}

	disconnect(connectionGeneration: number) {
		for (const actor of this.actors.values()) {
			actor.disconnect(connectionGeneration);
		}
		this.activeTerminalId = null;
	}

	removeTerminal(terminalId: string) {
		this.actors.get(terminalId)?.detach();
		this.actors.delete(terminalId);
		if (this.activeTerminalId === terminalId) {
			this.activeTerminalId = null;
		}
	}

	setActiveTerminalId(terminalId: string | null) {
		this.activeTerminalId = terminalId;
	}

	mountActive(host: HTMLElement) {
		if (this.activeTerminalId == null) {
			return Promise.resolve();
		}
		return this.actor(this.activeTerminalId).mount(host);
	}

	unmountActive() {
		if (this.activeTerminalId == null) {
			return;
		}
		this.actor(this.activeTerminalId).unmount();
	}

	resizeActive(cols: number, rows: number) {
		if (this.activeTerminalId == null) {
			return;
		}
		this.actor(this.activeTerminalId).resize(cols, rows);
	}

	loadOlderHistoryActive() {
		if (this.activeTerminalId == null) {
			return;
		}
		this.actor(this.activeTerminalId).loadOlderHistory();
	}

	private actor(terminalId: string) {
		const existing = this.actors.get(terminalId);
		if (existing != null) {
			return existing;
		}
		const actor = new TerminalSessionActor({
			terminalId,
			controller: this.deps.controller,
			workspaceState: this.deps.workspaceState,
			session: this.deps.session,
			onInlineError: this.deps.onInlineError,
		});
		this.actors.set(terminalId, actor);
		return actor;
	}
}
