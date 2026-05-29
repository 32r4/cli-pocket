import type { ClientBridge } from "@/platform/bridge/types";
import { TauriBridge } from "@/platform/tauri/TauriBridge";
import { WebBridge } from "@/platform/web/WebBridge";

export type AppPlatformId = "desktop" | "mobile" | "web";

export interface AppPlatform {
	id: AppPlatformId;
	shell: "desktop" | "mobile";
	bridge: "tauri" | "web";
	embeddedDaemon: boolean;
}

const PLATFORM_PROFILES: Record<AppPlatformId, AppPlatform> = {
	desktop: {
		id: "desktop",
		shell: "desktop",
		bridge: "tauri",
		embeddedDaemon: true,
	},
	mobile: {
		id: "mobile",
		shell: "mobile",
		bridge: "tauri",
		embeddedDaemon: false,
	},
	web: {
		id: "web",
		shell: "desktop",
		bridge: "web",
		embeddedDaemon: false,
	},
};

export function platformProfile(id: AppPlatformId): AppPlatform {
	return PLATFORM_PROFILES[id];
}

export const CURRENT_APP_PLATFORM = platformProfile(__APP_PLATFORM__);

export async function createBridgeForPlatform(
	platform: AppPlatform,
): Promise<ClientBridge> {
	if (platform.bridge === "tauri") {
		return new TauriBridge({ embeddedDaemon: platform.embeddedDaemon });
	}

	return WebBridge.create();
}
