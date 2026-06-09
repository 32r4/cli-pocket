import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HostSettingsSection } from "./HostSettingsSection";

afterEach(() => {
	cleanup();
});

function renderHostSettings({
	onScrollbackBytesChange = vi.fn(),
	onTerminalFontSizeChange = vi.fn(),
}: {
	onScrollbackBytesChange?: (scrollbackBytes: number) => void;
	onTerminalFontSizeChange?: (fontSize: number) => void;
} = {}) {
	return render(
		<HostSettingsSection
			scrollbackBytes={4 * 1024 * 1024}
			onScrollbackBytesChange={onScrollbackBytesChange}
			theme="dark"
			terminalFontSize={15}
			onTerminalFontSizeChange={onTerminalFontSizeChange}
			onCopyPairUrl={vi.fn()}
			isPairUrlCopied={false}
			showPairControls={false}
			onRestartLocalDaemon={vi.fn()}
			onThemeChange={vi.fn()}
		/>,
	);
}

describe("HostSettingsSection", () => {
	it("ignores empty terminal font size input", () => {
		const onTerminalFontSizeChange = vi.fn();
		const view = renderHostSettings({ onTerminalFontSizeChange });

		const input = view.getByLabelText("Terminal font size");
		fireEvent.change(input, { target: { value: "" } });
		fireEvent.blur(input);

		expect(onTerminalFontSizeChange).not.toHaveBeenCalledWith(Number.NaN);
		expect(onTerminalFontSizeChange).not.toHaveBeenCalled();
	});

	it("ignores empty scrollback input", () => {
		const onScrollbackBytesChange = vi.fn();
		const view = renderHostSettings({ onScrollbackBytesChange });

		const input = view.getByLabelText("Scrollback in MiB");
		fireEvent.change(input, { target: { value: "" } });
		fireEvent.blur(input);

		expect(onScrollbackBytesChange).not.toHaveBeenCalledWith(Number.NaN);
		expect(onScrollbackBytesChange).not.toHaveBeenCalled();
	});
});
