import { createStore } from "zustand/vanilla";

export type OverlaySection = "settings" | "diagnostics" | "about";
export type ThemeName = "light" | "dark";

const THEME_STORAGE_KEY = "cli-pocket-theme";

function isThemeName(value: string): value is ThemeName {
	return value === "light" || value === "dark";
}

function applyTheme(theme: ThemeName) {
	if (typeof document === "undefined") {
		return;
	}

	document.documentElement.dataset.theme = theme;
}

function readStoredTheme(): ThemeName {
	if (typeof window === "undefined") {
		return "dark";
	}

	const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
	return storedTheme != null && isThemeName(storedTheme) ? storedTheme : "dark";
}

function persistTheme(theme: ThemeName) {
	if (typeof window === "undefined") {
		return;
	}

	window.localStorage.setItem(THEME_STORAGE_KEY, theme);
}

interface UiState {
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
}

export function createUiStateStore() {
	const theme = readStoredTheme();
	applyTheme(theme);

	return createStore<UiState>((set) => ({
		isOverlayOpen: false,
		overlaySection: "settings",
		selectedServerId: null,
		isOverlayMenuRoot: true,
		theme,
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
		setTheme: (nextTheme) => {
			persistTheme(nextTheme);
			applyTheme(nextTheme);
			set({ theme: nextTheme });
		},
	}));
}
