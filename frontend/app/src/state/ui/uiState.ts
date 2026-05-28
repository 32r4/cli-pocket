import { createStore } from "zustand/vanilla";

export type OverlaySection = "settings" | "diagnostics" | "about";

interface UiState {
	isOverlayOpen: boolean;
	overlaySection: OverlaySection;
	selectedHostId: string | null;
	isOverlayMenuRoot: boolean;
	openOverlay: (section?: OverlaySection) => void;
	closeOverlay: () => void;
	setOverlaySection: (section: OverlaySection) => void;
	setSelectedHostId: (hostId: string | null) => void;
	showOverlayMenuRoot: () => void;
}

export function createUiStateStore() {
	return createStore<UiState>((set) => ({
		isOverlayOpen: false,
		overlaySection: "settings",
		selectedHostId: null,
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
		setSelectedHostId: (hostId) => set({ selectedHostId: hostId }),
		showOverlayMenuRoot: () => set({ isOverlayMenuRoot: true }),
	}));
}
