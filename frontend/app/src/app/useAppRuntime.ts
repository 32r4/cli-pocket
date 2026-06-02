import { useEffect, useRef, useState } from "react";
import { TerminalController } from "@/features/terminals/terminalController";
import type { PlatformServices, SessionActor } from "@/platform/bridge/types";
import type { AppPlatform } from "@/platform/runtime/platform";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { ConnectionController } from "./connectionController";
import { HostController } from "./hostController";
import type { AppStores } from "./stores";

interface UseAppRuntimeOptions {
	platform: AppPlatform;
	platformServicesFactory: (platform: AppPlatform) => Promise<PlatformServices>;
	stores: AppStores;
	onInlineError: (message: string | null) => void;
}

interface UseAppRuntimeResult {
	services: PlatformServices | null;
	platformError: string | null;
	session: SessionActor | null;
	terminalController: TerminalController;
	connectServer: (
		server: DaemonRecord,
		options?: { closeOverlay?: boolean },
	) => Promise<void>;
	disconnectCurrentServer: () => Promise<void>;
	copyLocalPairUrl: () => Promise<boolean>;
	restartLocalDaemon: () => Promise<void>;
	importPairingLink: (rawUrl: string) => Promise<void>;
}

export function useAppRuntime({
	platform,
	platformServicesFactory,
	stores,
	onInlineError,
}: UseAppRuntimeOptions): UseAppRuntimeResult {
	const { daemonRegistry, uiState, workspaceState } = stores;
	const [services, setServices] = useState<PlatformServices | null>(null);
	const [platformError, setPlatformError] = useState<string | null>(null);
	const [terminalController] = useState(
		() =>
			new TerminalController({
				onInput: () => undefined,
				onResize: () => undefined,
			}),
	);
	const hostControllerRef = useRef<HostController | null>(null);
	const controllerRef = useRef<ConnectionController | null>(null);

	useEffect(() => {
		let active = true;
		setServices(null);
		setPlatformError(null);

		void platformServicesFactory(platform)
			.then(async (instance) => {
				if (!active) {
					return;
				}

				setServices(instance);
				const hostController = new HostController({
					services: instance,
					daemonRegistry,
					uiState,
					onInlineError,
				});
				hostControllerRef.current = hostController;
				await hostController.bootstrap();
				if (!active) {
					return;
				}

				const controller = new ConnectionController({
					services: instance,
					daemonRegistry,
					uiState,
					workspaceState,
					onInlineError,
					onConnectionReset: () => terminalController.reset(),
					onTerminalOutput: (terminalId, chunk) => {
						terminalController.appendActiveOutput(terminalId, chunk);
					},
					onTerminalRemoved: (terminalId) => {
						terminalController.removeTerminal(terminalId);
					},
				});
				controllerRef.current = controller;
				await controller.bootstrap();

				if (!active) {
					await controller.shutdown();
				}
			})
			.catch((error: unknown) => {
				if (!active) {
					return;
				}
				setPlatformError(
					error instanceof Error
						? error.message
						: "failed to start platform services",
				);
			});

		return () => {
			active = false;
			hostControllerRef.current = null;
			const controller = controllerRef.current;
			controllerRef.current = null;
			if (controller != null) {
				void controller.shutdown();
			}
		};
	}, [
		daemonRegistry,
		onInlineError,
		platform,
		platformServicesFactory,
		terminalController,
		uiState,
		workspaceState,
	]);

	const connectServer = async (
		server: DaemonRecord,
		options?: { closeOverlay?: boolean },
	) => {
		try {
			await controllerRef.current?.connectServer(server, options);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "connection failed";
			workspaceState.getState().markConnectionFailed(message);
			onInlineError(message);
		}
	};

	const disconnectCurrentServer = async () => {
		await controllerRef.current?.disconnect();
	};

	const copyLocalPairUrl = async () => {
		const host = services?.host;
		if (host == null) {
			return false;
		}

		try {
			const nextPairUrl = await host.pairUrl();
			if (typeof navigator === "undefined" || navigator.clipboard == null) {
				throw new Error("clipboard unavailable");
			}
			await navigator.clipboard.writeText(nextPairUrl);
			onInlineError(null);
			return true;
		} catch (error: unknown) {
			onInlineError(
				error instanceof Error ? error.message : "failed to copy pair url",
			);
			return false;
		}
	};

	const restartLocalDaemon = async () => {
		await hostControllerRef.current?.restartLocalDaemon();
	};

	const importPairingLink = async (rawUrl: string) => {
		const importedServer =
			await hostControllerRef.current?.importPairingLink(rawUrl);
		if (importedServer != null) {
			await connectServer(importedServer);
		}
	};

	return {
		services,
		platformError,
		session: controllerRef.current?.getSession() ?? null,
		terminalController,
		connectServer,
		disconnectCurrentServer,
		copyLocalPairUrl,
		restartLocalDaemon,
		importPairingLink,
	};
}
