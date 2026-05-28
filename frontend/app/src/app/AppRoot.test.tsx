import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppRoot } from "./AppRoot";

describe("AppRoot", () => {
	it("renders the disconnected connection entry point", () => {
		render(<AppRoot clientKind="tauri" />);
		expect(
			screen.getByRole("heading", { name: "cli-pocket" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("heading", { name: "Connect to a host" }),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Menu" })).toBeInTheDocument();
		expect(screen.getByText("No saved hosts")).toBeInTheDocument();
	});
});
