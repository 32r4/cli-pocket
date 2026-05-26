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

	container.dataset.mobile = mobile ? "1" : "0";

	createRoot(container).render(
		<StrictMode>
			<AppRoot clientKind={clientKind} />
		</StrictMode>,
	);
}
