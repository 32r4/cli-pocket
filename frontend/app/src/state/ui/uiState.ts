import { createStore } from "zustand/vanilla";

export type OverlaySection = "host" | "settings" | "diagnostics" | "about";

interface UiState {
	isOverlayOpen: boolean;
	overlaySection: OverlaySection;
	selectedHostId: string | null;
	openOverlay: (section?: OverlaySection) => void;
	closeOverlay: () => void;
	setOverlaySection: (section: OverlaySection) => void;
	setSelectedHostId: (hostId: string | null) => void;
}

export function createUiStateStore() {
	return createStore<UiState>((set) => ({
		isOverlayOpen: false,
		overlaySection: "host",
		selectedHostId: null,
		openOverlay: (section = "host") =>
			set({
				isOverlayOpen: true,
				overlaySection: section,
			}),
		closeOverlay: () => set({ isOverlayOpen: false }),
		setOverlaySection: (section) => set({ overlaySection: section }),
		setSelectedHostId: (hostId) => set({ selectedHostId: hostId }),
	}));
}
