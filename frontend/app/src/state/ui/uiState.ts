import { createStore } from "zustand/vanilla";

export type MenuSection = "settings" | "diagnostics" | "about";
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
	isMenuOpen: boolean;
	menuSection: MenuSection;
	selectedServerId: string | null;
	isMenuRoot: boolean;
	theme: ThemeName;
	openMenu: (section?: MenuSection) => void;
	closeMenu: () => void;
	setMenuSection: (section: MenuSection) => void;
	setSelectedServerId: (serverId: string | null) => void;
	showMenuRoot: () => void;
	setTheme: (theme: ThemeName) => void;
}

export function createUiStateStore() {
	const theme = readStoredTheme();
	applyTheme(theme);

	return createStore<UiState>((set) => ({
		isMenuOpen: false,
		menuSection: "settings",
		selectedServerId: null,
		isMenuRoot: true,
		theme,
		openMenu: (section = "settings") =>
			set({
				isMenuOpen: true,
				menuSection: section,
				isMenuRoot: true,
			}),
		closeMenu: () => set({ isMenuOpen: false, isMenuRoot: true }),
		setMenuSection: (section) =>
			set({ menuSection: section, isMenuRoot: false }),
		setSelectedServerId: (serverId) => set({ selectedServerId: serverId }),
		showMenuRoot: () => set({ isMenuRoot: true }),
		setTheme: (nextTheme) => {
			persistTheme(nextTheme);
			applyTheme(nextTheme);
			set({ theme: nextTheme });
		},
	}));
}
