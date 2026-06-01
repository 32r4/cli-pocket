import { createStore } from "zustand/vanilla";

export type OverlaySection = "settings" | "diagnostics" | "about";
export type ThemeName = "light" | "dark";

const THEME_STORAGE_KEY = "cli-pocket-theme";
const SCROLLBACK_STORAGE_KEY = "cli-pocket-scrollback-bytes";
const DEFAULT_SCROLLBACK_BYTES = 4 * 1024 * 1024;
const MIN_SCROLLBACK_BYTES = 1 * 1024 * 1024;
const MAX_SCROLLBACK_BYTES = 64 * 1024 * 1024;

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

function clampScrollbackBytes(value: number) {
	return Math.min(MAX_SCROLLBACK_BYTES, Math.max(MIN_SCROLLBACK_BYTES, value));
}

function readStoredScrollbackBytes() {
	if (typeof window === "undefined") {
		return DEFAULT_SCROLLBACK_BYTES;
	}

	const storedScrollback = window.localStorage.getItem(SCROLLBACK_STORAGE_KEY);
	if (storedScrollback == null) {
		return DEFAULT_SCROLLBACK_BYTES;
	}

	const parsed = Number(storedScrollback);
	return Number.isFinite(parsed)
		? clampScrollbackBytes(Math.round(parsed))
		: DEFAULT_SCROLLBACK_BYTES;
}

function persistScrollbackBytes(scrollbackBytes: number) {
	if (typeof window === "undefined") {
		return;
	}

	window.localStorage.setItem(
		SCROLLBACK_STORAGE_KEY,
		String(clampScrollbackBytes(scrollbackBytes)),
	);
}

interface UiState {
	isOverlayOpen: boolean;
	overlaySection: OverlaySection;
	selectedServerId: string | null;
	isOverlayMenuRoot: boolean;
	theme: ThemeName;
	scrollbackBytes: number;
	openOverlay: (section?: OverlaySection) => void;
	closeOverlay: () => void;
	setOverlaySection: (section: OverlaySection) => void;
	setSelectedServerId: (serverId: string | null) => void;
	showOverlayMenuRoot: () => void;
	setTheme: (theme: ThemeName) => void;
	setScrollbackBytes: (scrollbackBytes: number) => void;
}

export function createUiStateStore() {
	const theme = readStoredTheme();
	const scrollbackBytes = readStoredScrollbackBytes();
	applyTheme(theme);

	return createStore<UiState>((set) => ({
		isOverlayOpen: false,
		overlaySection: "settings",
		selectedServerId: null,
		isOverlayMenuRoot: true,
		theme,
		scrollbackBytes,
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
		setScrollbackBytes: (nextScrollbackBytes) => {
			const nextValue = clampScrollbackBytes(nextScrollbackBytes);
			persistScrollbackBytes(nextValue);
			set({ scrollbackBytes: nextValue });
		},
	}));
}
