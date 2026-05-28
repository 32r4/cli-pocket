import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppRoot } from "./AppRoot";

describe("AppRoot", () => {
	it("renders the disconnected host entry point", () => {
		render(<AppRoot clientKind="tauri" />);
		expect(screen.getByText("No host")).toBeInTheDocument();
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
