import { z } from "zod";
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

const STORAGE_KEY = "cli-pocket/daemon-registry/v1";

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

function loadPersistedState() {
	if (typeof window === "undefined") {
		return {
			daemons: [] as DaemonRecord[],
			selectedDaemonId: null as string | null,
		};
	}

	try {
		const raw = window.localStorage.getItem(STORAGE_KEY);
		if (raw == null) {
			return {
				daemons: [] as DaemonRecord[],
				selectedDaemonId: null as string | null,
			};
		}

		const parsed = PersistedDaemonRegistrySchema.safeParse(JSON.parse(raw));
		if (!parsed.success) {
			return {
				daemons: [] as DaemonRecord[],
				selectedDaemonId: null as string | null,
			};
		}

		return {
			daemons: parsed.data.daemons,
			selectedDaemonId: parsed.data.selectedDaemonId,
		};
	} catch {
		return {
			daemons: [] as DaemonRecord[],
			selectedDaemonId: null as string | null,
		};
	}
}

function persistState(
	state: Pick<DaemonRegistryState, "daemons" | "selectedDaemonId">,
) {
	if (typeof window === "undefined") {
		return;
	}

	try {
		window.localStorage.setItem(
			STORAGE_KEY,
			JSON.stringify({
				version: 1,
				daemons: state.daemons,
				selectedDaemonId: state.selectedDaemonId,
			}),
		);
	} catch {}
}

export function createDaemonRegistryStore() {
	const initialState = loadPersistedState();
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
	}));

	store.subscribe((state) => {
		persistState({
			daemons: state.daemons,
			selectedDaemonId: state.selectedDaemonId,
		});
	});

	return store;
}
