import { createStore } from "zustand/vanilla";
import type { AppRoute } from "@/app/router/routes";

interface UiState {
	route: AppRoute;
	setRoute: (route: AppRoute) => void;
}

export function createUiStateStore(initialRoute: AppRoute) {
	return createStore<UiState>((set) => ({
		route: initialRoute,
		setRoute: (route) => set({ route }),
	}));
}
