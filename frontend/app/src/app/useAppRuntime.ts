import { useEffect, useRef, useState } from "react";
import { TerminalController } from "@/features/terminals/terminalController";
import { TerminalSessionRegistry } from "@/features/terminals/terminalSessionRegistry";
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
	terminalRegistry: TerminalSessionRegistry;
	connectServer: (
		server: DaemonRecord,
		options?: { closeMenu?: boolean },
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
	const hostControllerRef = useRef<HostController | null>(null);
	const controllerRef = useRef<ConnectionController | null>(null);
	const terminalRegistryRef = useRef<TerminalSessionRegistry | null>(null);
	const [terminalController] = useState(
		() =>
			new TerminalController({
				onInput: (terminalId, data) => {
					const session = controllerRef.current?.getSession() ?? null;
					if (session == null) {
						return;
					}
					void session
						.sendInput(terminalId, new TextEncoder().encode(data))
						.catch((error: unknown) => {
							onInlineError(
								error instanceof Error ? error.message : "failed to send input",
							);
						});
				},
				onResize: (_terminalId, cols, rows) => {
					terminalRegistryRef.current?.resizeActive(cols, rows);
				},
				onLoadOlderHistory: () => {
					terminalRegistryRef.current?.loadOlderHistoryActive();
				},
			}),
	);
	const [terminalRegistry] = useState(() => {
		const registry = new TerminalSessionRegistry({
			controller: terminalController,
			workspaceState,
			session: () => controllerRef.current?.getSession() ?? null,
			onInlineError,
		});
		terminalRegistryRef.current = registry;
		return registry;
	});

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
					onTerminalRemoved: (terminalId) => {
						terminalController.removeTerminal(terminalId);
					},
					terminalRegistry,
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
			terminalRegistry.dispose();
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
		terminalRegistry,
		uiState,
		workspaceState,
	]);

	const connectServer = async (
		server: DaemonRecord,
		options?: { closeMenu?: boolean },
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
		terminalRegistry,
		connectServer,
		disconnectCurrentServer,
		copyLocalPairUrl,
		restartLocalDaemon,
		importPairingLink,
	};
}
