import { createStore } from "zustand/vanilla";

export type MenuSection = "settings" | "diagnostics" | "about";
export type ThemeName = "light" | "dark";

const THEME_STORAGE_KEY = "cli-pocket-theme";
const TERMINAL_FONT_SIZE_STORAGE_KEY = "cli-pocket-terminal-font-size";
export const MIN_TERMINAL_FONT_SIZE = 10;
export const MAX_TERMINAL_FONT_SIZE = 20;
export const DEFAULT_TERMINAL_FONT_SIZE = 15;

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

function clampTerminalFontSize(value: number) {
	return Math.min(
		MAX_TERMINAL_FONT_SIZE,
		Math.max(MIN_TERMINAL_FONT_SIZE, value),
	);
}

function isTerminalFontSize(value: number) {
	return (
		Number.isInteger(value) &&
		value >= MIN_TERMINAL_FONT_SIZE &&
		value <= MAX_TERMINAL_FONT_SIZE
	);
}

function readStoredTerminalFontSize() {
	if (typeof window === "undefined") {
		return DEFAULT_TERMINAL_FONT_SIZE;
	}

	const storedFontSize = Number.parseInt(
		window.localStorage.getItem(TERMINAL_FONT_SIZE_STORAGE_KEY) ?? "",
		10,
	);
	return isTerminalFontSize(storedFontSize)
		? storedFontSize
		: DEFAULT_TERMINAL_FONT_SIZE;
}

function persistTerminalFontSize(fontSize: number) {
	if (typeof window === "undefined") {
		return;
	}

	window.localStorage.setItem(TERMINAL_FONT_SIZE_STORAGE_KEY, String(fontSize));
}

interface UiState {
	isMenuOpen: boolean;
	menuSection: MenuSection;
	selectedServerId: string | null;
	isMenuRoot: boolean;
	theme: ThemeName;
	terminalFontSize: number;
	openMenu: (section?: MenuSection) => void;
	closeMenu: () => void;
	setMenuSection: (section: MenuSection) => void;
	setSelectedServerId: (serverId: string | null) => void;
	showMenuRoot: () => void;
	setTheme: (theme: ThemeName) => void;
	setTerminalFontSize: (fontSize: number) => void;
}

export function createUiStateStore() {
	const theme = readStoredTheme();
	const terminalFontSize = readStoredTerminalFontSize();
	applyTheme(theme);

	return createStore<UiState>((set) => ({
		isMenuOpen: false,
		menuSection: "settings",
		selectedServerId: null,
		isMenuRoot: true,
		theme,
		terminalFontSize,
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
		setTerminalFontSize: (nextFontSize) => {
			const clampedFontSize = clampTerminalFontSize(nextFontSize);
			persistTerminalFontSize(clampedFontSize);
			set({ terminalFontSize: clampedFontSize });
		},
	}));
}
