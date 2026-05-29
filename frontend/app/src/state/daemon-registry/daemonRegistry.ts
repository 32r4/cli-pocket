import { z } from "zod";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { DaemonRecord } from "./types";

export interface PersistedDaemonRegistry {
	version: 1;
	daemons: DaemonRecord[];
	selectedDaemonId: string | null;
}

export interface DaemonRegistryPersistence {
	load(): Promise<PersistedDaemonRegistry | null>;
	save(state: PersistedDaemonRegistry): Promise<void>;
}

export interface DaemonRegistryState {
	daemons: DaemonRecord[];
	selectedDaemonId: string | null;
	upsertDaemon: (daemon: DaemonRecord) => void;
	updateDaemonLabel: (id: string, label: string) => void;
	selectDaemon: (id: string | null) => void;
	removeDaemon: (id: string) => void;
	replacePersistedState: (state: PersistedDaemonRegistry) => void;
}

export interface DaemonRegistryStore extends StoreApi<DaemonRegistryState> {
	snapshotPersistedState(): PersistedDaemonRegistry;
	hydratePersistedState(state: PersistedDaemonRegistry): void;
	setPersistence(persistence: DaemonRegistryPersistence): void;
}

const DirectDaemonRecordSchema = z.object({
	id: z.string(),
	label: z.string(),
	resumeTokenHex: z.string().nullable(),
	lastConnectedAt: z.number().nullable(),
	kind: z.literal("direct"),
	endpointUrl: z.string(),
});

const RelayDaemonRecordSchema = z.object({
	id: z.string(),
	label: z.string(),
	resumeTokenHex: z.string().nullable(),
	lastConnectedAt: z.number().nullable(),
	kind: z.literal("relay"),
	serverPublicHex: z.string(),
	serverId: z.string(),
	relayUrl: z.string(),
	relayPskHex: z.string(),
});

const PersistedDaemonRegistrySchema = z.object({
	version: z.literal(1),
	daemons: z.array(
		z.union([DirectDaemonRecordSchema, RelayDaemonRecordSchema]),
	),
	selectedDaemonId: z.string().nullable(),
});

export function emptyPersistedDaemonRegistry(): PersistedDaemonRegistry {
	return {
		version: 1,
		daemons: [],
		selectedDaemonId: null,
	};
}

export function parsePersistedDaemonRegistry(
	value: unknown,
): PersistedDaemonRegistry | null {
	const parsed = PersistedDaemonRegistrySchema.safeParse(value);
	if (!parsed.success) {
		return null;
	}

	return parsed.data;
}

function snapshotPersistedState(
	state: Pick<DaemonRegistryState, "daemons" | "selectedDaemonId">,
): PersistedDaemonRegistry {
	return {
		version: 1,
		daemons: state.daemons,
		selectedDaemonId: state.selectedDaemonId,
	};
}

export function createDaemonRegistryStore(): DaemonRegistryStore {
	const initialState = emptyPersistedDaemonRegistry();
	let persistence: DaemonRegistryPersistence | null = null;
	let skipNextPersist = false;
	const store = createStore<DaemonRegistryState>((set) => ({
		daemons: initialState.daemons,
		selectedDaemonId: initialState.selectedDaemonId,
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
		replacePersistedState: (state) =>
			set({
				daemons: state.daemons,
				selectedDaemonId: state.selectedDaemonId,
			}),
	}));

	store.subscribe((state) => {
		if (skipNextPersist) {
			skipNextPersist = false;
			return;
		}

		if (persistence == null) {
			return;
		}

		void persistence.save(snapshotPersistedState(state));
	});

	return Object.assign(store, {
		snapshotPersistedState() {
			return snapshotPersistedState(store.getState());
		},
		hydratePersistedState(state: PersistedDaemonRegistry) {
			skipNextPersist = true;
			store.getState().replacePersistedState(state);
		},
		setPersistence(nextPersistence: DaemonRegistryPersistence) {
			persistence = nextPersistence;
		},
	});
}
