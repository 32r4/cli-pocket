import type { StoreApi } from "zustand/vanilla";
import type { SessionActor } from "@/platform/bridge/types";
import type { TerminalController } from "./terminalController";
import {
	type TerminalRuntimeState,
	TerminalSessionActor,
} from "./terminalSessionActor";

interface WorkspaceStoreShape {
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
	private runtimeStates = new Map<string, TerminalRuntimeState>();
	private selectedTerminalId: string | null = null;
	private connectionGeneration: number | null = null;

	constructor(private readonly deps: TerminalSessionRegistryDeps) {}

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

	connect(connectionGeneration: number) {
		this.connectionGeneration = connectionGeneration;
		this.activateSelectedTerminal();
	}

	disconnect(connectionGeneration: number) {
		for (const actor of this.actors.values()) {
			actor.disconnect(connectionGeneration);
		}
		this.connectionGeneration = null;
	}

	removeTerminal(terminalId: string) {
		this.actors.get(terminalId)?.detach();
		this.actors.delete(terminalId);
		this.runtimeStates.delete(terminalId);
		if (this.selectedTerminalId === terminalId) {
			this.selectedTerminalId = null;
		}
	}

	dispose() {}

	mountActive(host: HTMLElement) {
		if (this.selectedTerminalId == null) {
			return Promise.resolve();
		}
		return this.actor(this.selectedTerminalId).mount(host);
	}

	unmountActive() {
		if (this.selectedTerminalId == null) {
			return;
		}
		this.actor(this.selectedTerminalId).unmount();
	}

	resizeActive(cols: number, rows: number) {
		if (this.selectedTerminalId == null) {
			return;
		}
		this.actor(this.selectedTerminalId).resize(cols, rows);
	}

	loadOlderHistoryActive() {
		if (this.selectedTerminalId == null) {
			return;
		}
		this.actor(this.selectedTerminalId).loadOlderHistory();
	}

	activeRuntimeState() {
		if (this.selectedTerminalId == null) {
			return null;
		}
		return this.runtimeStates.get(this.selectedTerminalId) ?? null;
	}

	setSelectedTerminal(terminalId: string | null) {
		if (
			this.selectedTerminalId != null &&
			this.selectedTerminalId !== terminalId
		) {
			this.actors.get(this.selectedTerminalId)?.detach();
		}
		this.selectedTerminalId = terminalId;
		this.activateSelectedTerminal();
	}

	private activateSelectedTerminal() {
		if (this.selectedTerminalId == null || this.connectionGeneration == null) {
			return;
		}
		this.actor(this.selectedTerminalId).activateTerminal(
			this.connectionGeneration,
		);
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
			onRuntimeStateChange: (updatedTerminalId, runtimeState) => {
				this.runtimeStates.set(updatedTerminalId, runtimeState);
			},
		});
		this.actors.set(terminalId, actor);
		this.runtimeStates.set(terminalId, actor.getRuntimeState());
		return actor;
	}
}
