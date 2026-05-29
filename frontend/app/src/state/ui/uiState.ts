import { createStore } from "zustand/vanilla";

export type OverlaySection = "settings" | "diagnostics" | "about";

interface UiState {
	isOverlayOpen: boolean;
	overlaySection: OverlaySection;
	selectedServerId: string | null;
	isOverlayMenuRoot: boolean;
	openOverlay: (section?: OverlaySection) => void;
	closeOverlay: () => void;
	setOverlaySection: (section: OverlaySection) => void;
	setSelectedServerId: (serverId: string | null) => void;
	showOverlayMenuRoot: () => void;
}

export function createUiStateStore() {
	return createStore<UiState>((set) => ({
		isOverlayOpen: false,
		overlaySection: "settings",
		selectedServerId: null,
		isOverlayMenuRoot: true,
		openOverlay: (section = "settings") =>
			set({
				isOverlayOpen: true,
				overlaySection: section,
				isOverlayMenuRoot: true,
			}),
		closeOverlay: () => set({ isOverlayOpen: false, isOverlayMenuRoot: true }),
		setOverlaySection: (section) =>
			set({ overlaySection: section, isOverlayMenuRoot: false }),
		setSelectedServerId: (serverId) => set({ selectedServerId: serverId }),
		showOverlayMenuRoot: () => set({ isOverlayMenuRoot: true }),
	}));
}
