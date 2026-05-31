import type { StoreApi } from "zustand/vanilla";
import { importPairingOfferUrl } from "@/features/pairing/pairingOffer";
import type { PlatformServices } from "@/platform/bridge/types";
import type {
	DaemonRegistryStore,
	PersistedDaemonRegistry,
} from "@/state/daemon-registry/daemonRegistry";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import type { OverlaySection, ThemeName } from "@/state/ui/uiState";

type UiStateStore = StoreApi<{
	isOverlayOpen: boolean;
	overlaySection: OverlaySection;
	selectedServerId: string | null;
	isOverlayMenuRoot: boolean;
	theme: ThemeName;
	openOverlay: (section?: OverlaySection) => void;
	closeOverlay: () => void;
	setOverlaySection: (section: OverlaySection) => void;
	setSelectedServerId: (serverId: string | null) => void;
	showOverlayMenuRoot: () => void;
	setTheme: (theme: ThemeName) => void;
}>;

interface HostControllerDeps {
	services: PlatformServices;
	daemonRegistry: DaemonRegistryStore;
	uiState: UiStateStore;
	onInlineError: (message: string | null) => void;
}

function currentPairingUrlFromLocation() {
	if (typeof window === "undefined") {
		return null;
	}

	const fragment = window.location.hash.startsWith("#")
		? window.location.hash.slice(1)
		: window.location.hash;
	if (new URLSearchParams(fragment).get("pair") == null) {
		return null;
	}

	return window.location.href;
}

function clearLocationHash() {
	if (typeof window === "undefined" || window.location.hash.length === 0) {
		return;
	}

	const nextUrl =
		window.location.search.length > 0
			? `${window.location.pathname}${window.location.search}`
			: window.location.pathname;
	window.history.replaceState(null, "", nextUrl);
}

export class HostController {
	private bootstrapped = false;

	constructor(private readonly deps: HostControllerDeps) {}

	async bootstrap() {
		if (this.bootstrapped) {
			return;
		}
		this.bootstrapped = true;

		await this.restoreDaemonRegistry();
		await this.registerEmbeddedDaemon();
		await this.importPairingUrlFromLocation();
	}

	async importPairingLink(rawUrl: string) {
		const importedServer = importPairingOfferUrl(rawUrl);
		this.deps.daemonRegistry.getState().upsertDaemon(importedServer);
		this.deps.daemonRegistry.getState().selectDaemon(importedServer.id);
		this.deps.uiState.getState().setSelectedServerId(importedServer.id);
		this.deps.onInlineError(null);

		return importedServer;
	}

	async generateLocalPairUrl(): Promise<string | null> {
		const host = this.deps.services.host;
		if (host == null) {
			return null;
		}

		try {
			const localPairUrl = await host.pairUrl();
			this.deps.onInlineError(null);
			return localPairUrl;
		} catch (error: unknown) {
			this.deps.onInlineError(
				error instanceof Error ? error.message : "failed to generate pair url",
			);
			return null;
		}
	}

	async restartLocalDaemon() {
		const host = this.deps.services.host;
		if (host == null) {
			return;
		}

		try {
			await host.restart();
			this.deps.onInlineError(null);
		} catch (error: unknown) {
			this.deps.onInlineError(
				error instanceof Error ? error.message : "failed to restart daemon",
			);
		}
	}

	private async restoreDaemonRegistry() {
		const persistence = {
			load: () => this.deps.services.registry.load(),
			save: (state: PersistedDaemonRegistry) =>
				this.deps.services.registry.save(state),
		};
		this.deps.daemonRegistry.setPersistence(persistence);

		try {
			const state = await persistence.load();
			this.deps.daemonRegistry.hydratePersistedState(
				state ?? {
					version: 1,
					daemons: [],
					selectedDaemonId: null,
				},
			);
		} catch (error: unknown) {
			this.deps.daemonRegistry.hydratePersistedState({
				version: 1,
				daemons: [],
				selectedDaemonId: null,
			});
			this.deps.onInlineError(
				error instanceof Error
					? error.message
					: "failed to restore saved servers",
			);
		}

		this.reconcileSelectedServer();
	}

	private async registerEmbeddedDaemon() {
		const host = this.deps.services.host;
		if (host == null) {
			return;
		}

		try {
			const endpointUrl = await host.localEndpoint();
			const existing = this.deps.daemonRegistry
				.getState()
				.daemons.find((daemon) => daemon.id === "local-daemon");
			const localDaemon: DaemonRecord = {
				id: "local-daemon",
				label: existing?.label ?? "This desktop",
				kind: "direct",
				endpointUrl,
				resumeTokenHex: existing?.resumeTokenHex ?? null,
				lastConnectedAt: existing?.lastConnectedAt ?? null,
			};
			this.deps.daemonRegistry.getState().upsertDaemon(localDaemon);
			this.deps.daemonRegistry.getState().selectDaemon(localDaemon.id);
			this.deps.uiState.getState().setSelectedServerId(localDaemon.id);
		} catch (error: unknown) {
			this.deps.onInlineError(
				error instanceof Error
					? error.message
					: "failed to resolve local daemon endpoint",
			);
		}
	}

	private async importPairingUrlFromLocation() {
		const rawUrl = currentPairingUrlFromLocation();
		if (rawUrl == null) {
			return;
		}

		this.deps.onInlineError(null);
		try {
			await this.importPairingLink(rawUrl);
		} catch (error: unknown) {
			this.deps.onInlineError(
				error instanceof Error
					? error.message
					: "failed to import pairing link",
			);
		} finally {
			clearLocationHash();
		}
	}

	private reconcileSelectedServer() {
		const registry = this.deps.daemonRegistry.getState();
		const ui = this.deps.uiState.getState();
		const fallbackServerId =
			registry.selectedDaemonId ?? registry.daemons[0]?.id ?? null;
		const selectedServerStillExists = registry.daemons.some(
			(daemon) => daemon.id === ui.selectedServerId,
		);

		if (
			!selectedServerStillExists &&
			ui.selectedServerId !== fallbackServerId
		) {
			this.deps.uiState.getState().setSelectedServerId(fallbackServerId);
		}

		if (registry.selectedDaemonId == null && fallbackServerId != null) {
			this.deps.daemonRegistry.getState().selectDaemon(fallbackServerId);
		}
	}
}
