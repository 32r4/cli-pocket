import { describe, expect, it, vi } from "vitest";
import type { PlatformServices } from "@/platform/bridge/types";
import { createDaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import { createUiStateStore } from "@/state/ui/uiState";
import { HostController } from "./hostController";

function makeServices(overrides?: Partial<PlatformServices>): PlatformServices {
	return {
		sessionFactory: {
			connect: vi.fn(),
		},
		registry: {
			load: vi.fn(async () => ({
				version: 1 as const,
				daemons: [],
				selectedDaemonId: null,
			})),
			save: vi.fn(async () => undefined),
			exportIdentity: vi.fn(async () => new Uint8Array()),
			importIdentity: vi.fn(async () => undefined),
		},
		host: {
			localEndpoint: vi.fn(async () => "ws://127.0.0.1:9999"),
			pairUrl: vi.fn(async () => "https://example.test/#pair=abc"),
			restart: vi.fn(async () => undefined),
		},
		...overrides,
	};
}

describe("HostController", () => {
	it("owns daemon restore and embedded daemon registration", async () => {
		const services = makeServices({
			registry: {
				load: vi.fn(async () => ({
					version: 1 as const,
					daemons: [
						{
							id: "saved",
							label: "saved",
							kind: "direct" as const,
							endpointUrl: "ws://saved",
							resumeTokenHex: null,
							lastConnectedAt: null,
						},
					],
					selectedDaemonId: "saved",
				})),
				save: vi.fn(async () => undefined),
				exportIdentity: vi.fn(async () => new Uint8Array()),
				importIdentity: vi.fn(async () => undefined),
			},
		});
		const daemonRegistry = createDaemonRegistryStore();
		const uiState = createUiStateStore();
		const onInlineError = vi.fn();
		const controller = new HostController({
			services,
			daemonRegistry,
			uiState,
			onInlineError,
		});

		await controller.bootstrap();

		expect(
			daemonRegistry.getState().daemons.map((daemon) => daemon.id),
		).toEqual(["saved", "local-daemon"]);
		expect(uiState.getState().selectedServerId).toBe("local-daemon");
		expect(onInlineError).not.toHaveBeenCalledWith(expect.any(String));
	});

	it("owns daemon restart", async () => {
		const services = makeServices();
		const controller = new HostController({
			services,
			daemonRegistry: createDaemonRegistryStore(),
			uiState: createUiStateStore(),
			onInlineError: vi.fn(),
		});

		await controller.restartLocalDaemon();

		expect(services.host?.restart).toHaveBeenCalledTimes(1);
	});
});
