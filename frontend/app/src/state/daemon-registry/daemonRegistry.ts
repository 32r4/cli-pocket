import { createStore } from "zustand/vanilla";
import type { DaemonRecord } from "./types";

interface DaemonRegistryState {
	daemons: DaemonRecord[];
	selectedDaemonId: string | null;
	upsertDaemon: (daemon: DaemonRecord) => void;
	updateDaemonLabel: (id: string, label: string) => void;
	selectDaemon: (id: string | null) => void;
	removeDaemon: (id: string) => void;
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
		updateDaemonLabel: (id, label) =>
			set((state) => ({
				daemons: state.daemons.map((daemon) =>
					daemon.id === id ? { ...daemon, label } : daemon,
				),
			})),
		selectDaemon: (id) => set({ selectedDaemonId: id }),
		removeDaemon: (id) =>
			set((state) => {
				const daemons = state.daemons.filter((daemon) => daemon.id !== id);
				const selectedDaemonId =
					state.selectedDaemonId === id
						? (daemons[0]?.id ?? null)
						: state.selectedDaemonId;

				return {
					daemons,
					selectedDaemonId,
				};
			}),
	}));
}
