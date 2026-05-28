import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot } from "../AppRoot";

export interface MountOptions {
	clientKind: "web" | "tauri";
	mobile: boolean;
}

export function mountApp({ clientKind, mobile }: MountOptions) {
	const container = document.getElementById("root");
	if (container === null) {
		throw new Error("missing #root");
	}

	createRoot(container).render(
		<StrictMode>
			<AppRoot clientKind={clientKind} mobile={mobile} />
		</StrictMode>,
	);
}
