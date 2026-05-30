import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { CURRENT_APP_PLATFORM } from "@/platform/runtime/platform";
import { AppRoot } from "../AppRoot";

export function mountApp() {
	const container = document.getElementById("root");
	if (container === null) {
		throw new Error("missing #root");
	}

	createRoot(container).render(
		CURRENT_APP_PLATFORM.bridge === "web" ? (
			<AppRoot platform={CURRENT_APP_PLATFORM} />
		) : (
			<StrictMode>
				<AppRoot platform={CURRENT_APP_PLATFORM} />
			</StrictMode>
		),
	);
}
