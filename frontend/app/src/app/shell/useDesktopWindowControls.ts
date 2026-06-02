import { getCurrentWindow } from "@tauri-apps/api/window";

export interface DesktopWindowControls {
	minimize: () => void;
	toggleMaximize: () => void;
	close: () => void;
	startDragging: () => void;
}

export function useDesktopWindowControls(
	enabled: boolean,
): DesktopWindowControls | null {
	if (!enabled) {
		return null;
	}

	const appWindow = getCurrentWindow();

	return {
		minimize: () => {
			void appWindow.minimize();
		},
		toggleMaximize: () => {
			void appWindow.toggleMaximize();
		},
		close: () => {
			void appWindow.close();
		},
		startDragging: () => {
			void appWindow.startDragging();
		},
	};
}
