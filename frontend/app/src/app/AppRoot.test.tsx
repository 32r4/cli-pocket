import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
	ConnectConfig,
	PlatformServices,
	SessionActor,
} from "@/platform/bridge/types";
import type { AppPlatform } from "@/platform/runtime/platform";
import { AppRoot } from "./AppRoot";

afterEach(() => {
	cleanup();
});

const webPlatform: AppPlatform = {
	id: "web",
	shell: "desktop",
	bridge: "web",
	embeddedDaemon: false,
};

function makeServices(connect: PlatformServices["sessionFactory"]["connect"]) {
	return {
		sessionFactory: { connect },
		registry: {
			load: vi.fn(async () => ({
				version: 1 as const,
				daemons: [
					{
						id: "server-a",
						label: "server-a",
						kind: "direct" as const,
						endpointUrl: "ws://127.0.0.1:9999",
						resumeTokenHex: null,
						lastConnectedAt: null,
					},
				],
				selectedDaemonId: "server-a",
			})),
			save: vi.fn(async () => undefined),
			exportIdentity: vi.fn(async () => new Uint8Array()),
			importIdentity: vi.fn(async () => undefined),
		},
		host: null,
	} satisfies PlatformServices;
}

describe("AppRoot", () => {
	it("shows only retry for connection failures and retries the selected server", async () => {
		const connect = vi
			.fn<(config: ConnectConfig) => Promise<SessionActor>>()
			.mockRejectedValueOnce(new Error("server unavailable"))
			.mockImplementation(() => new Promise<SessionActor>(() => undefined));
		const services = makeServices(connect);

		const view = render(
			<AppRoot
				platform={webPlatform}
				platformServicesFactory={async () => services}
			/>,
		);

		const retry = await view.findByRole("button", { name: "Retry" });
		expect(view.queryByRole("alert")).toBeNull();
		expect(view.queryByText("Connection failed")).toBeNull();

		fireEvent.click(retry);

		await waitFor(() => {
			expect(connect).toHaveBeenCalledTimes(2);
		});
		expect(connect.mock.calls[1]?.[0]).toEqual({
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:9999",
			resumeTokenHex: undefined,
		});
	});
});
