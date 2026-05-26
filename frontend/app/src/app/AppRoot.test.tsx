import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppRoot } from "./AppRoot";

describe("AppRoot", () => {
	it("labels web and tauri entries through the mounted app", () => {
		render(<AppRoot clientKind="tauri" />);
		expect(screen.getByText("client kind: tauri")).toBeInTheDocument();
	});
});
