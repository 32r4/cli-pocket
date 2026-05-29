import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { platformProfile } from "@/platform/runtime/platform";
import { AppRoot } from "./AppRoot";

describe("AppRoot", () => {
	it("renders the disconnected server entry point", async () => {
		render(
			<AppRoot
				platform={platformProfile("web")}
				bridgeFactory={async () => ({
					connect: async () => {},
					events: () => (async function* () {})(),
					createTerminal: async () => {},
					sendInput: async () => {},
					resize: async () => {},
					kill: async () => {},
					exportIdentity: async () => new Uint8Array(),
					importIdentity: async () => {},
					daemonRegistry: {
						load: async () => null,
						save: async () => {},
					},
					embeddedDaemon: null,
					close: async () => {},
				})}
			/>,
		);

		expect(await screen.findByText("No server")).toBeInTheDocument();
		expect(screen.getByLabelText("Status red")).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Direct connection" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Pairing link" }),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "QR code" })).toBeDisabled();
		expect(
			screen.getByRole("button", { name: "Open control overlay" }),
		).toBeInTheDocument();
	});
});
