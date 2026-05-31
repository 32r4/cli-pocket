import { createDaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import { createUiStateStore } from "@/state/ui/uiState";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";

export interface AppStores {
	daemonRegistry: ReturnType<typeof createDaemonRegistryStore>;
	uiState: ReturnType<typeof createUiStateStore>;
	workspaceState: ReturnType<typeof createWorkspaceStore>;
}

export function createAppStores(): AppStores {
	return {
		daemonRegistry: createDaemonRegistryStore(),
		uiState: createUiStateStore(),
		workspaceState: createWorkspaceStore(),
	};
}
