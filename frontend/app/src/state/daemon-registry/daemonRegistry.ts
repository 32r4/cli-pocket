import { createStore } from "zustand/vanilla";
import type { DaemonRecord } from "./types";

interface DaemonRegistryState {
	daemons: DaemonRecord[];
	selectedDaemonId: string | null;
	upsertDaemon: (daemon: DaemonRecord) => void;
	selectDaemon: (id: string | null) => void;
}

export function createDaemonRegistryStore() {
	return createStore<DaemonRegistryState>((set) => ({
		daemons: [],
		selectedDaemonId: null,
		upsertDaemon: (daemon) =>
			set((state) => {
				const existing = state.daemons.findIndex(
					(item) => item.id === daemon.id,
				);
				if (existing === -1) {
					return { daemons: [...state.daemons, daemon] };
				}

				const next = state.daemons.slice();
				next[existing] = daemon;
				return { daemons: next };
			}),
		selectDaemon: (id) => set({ selectedDaemonId: id }),
	}));
}
