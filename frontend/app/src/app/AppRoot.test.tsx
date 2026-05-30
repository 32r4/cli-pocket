import { cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { platformProfile } from "@/platform/runtime/platform";
import { AppRoot } from "./AppRoot";

afterEach(() => {
	cleanup();
	window.history.replaceState(null, "", "/");
});

describe("AppRoot", () => {
	it("exports a component function", () => {
		expect(typeof AppRoot).toBe("function");
	});

	it("uses the web platform profile in tests", () => {
		expect(platformProfile("web").bridge).toBe("web");
	});
});
