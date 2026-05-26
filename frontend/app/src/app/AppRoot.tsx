import { DaemonListScreen } from "@/features/daemons/DaemonListScreen";
import { PairingScreen } from "@/features/pairing/PairingScreen";
import { SettingsScreen } from "@/features/settings/SettingsScreen";
import { TerminalWorkspace } from "@/features/terminals/TerminalWorkspace";
import { createDaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import { Shell } from "./shell/Shell";

const daemonRegistry = createDaemonRegistryStore();

export function AppRoot({ clientKind }: { clientKind: "web" | "tauri" }) {
	const state = daemonRegistry.getState();

	return (
		<Shell>
			<p>client kind: {clientKind}</p>
			<DaemonListScreen daemons={state.daemons} />
			<PairingScreen />
			<TerminalWorkspace />
			<SettingsScreen />
		</Shell>
	);
}
