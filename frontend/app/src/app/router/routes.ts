export type AppRoute = "daemons" | "pairing" | "workspace" | "settings";

export function routeFor(selectedDaemonId: string | null): AppRoute {
	return selectedDaemonId === null ? "daemons" : "workspace";
}
