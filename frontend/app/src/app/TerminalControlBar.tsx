import type { PointerEvent } from "react";
import type { TerminalController } from "@/features/terminals/terminalController";
import {
	sequenceForControlKey,
	type TerminalModifierKey,
} from "@/features/terminals/terminalInput";

interface TerminalControlBarProps {
	controller: TerminalController;
	modifiers: ReadonlySet<TerminalModifierKey>;
	onInlineError: (message: string | null) => void;
	onClearModifiers: () => void;
	onToggleModifier: (modifier: TerminalModifierKey) => void;
}

export function TerminalControlBar({
	controller,
	modifiers,
	onInlineError,
	onClearModifiers,
	onToggleModifier,
}: TerminalControlBarProps) {
	const send = (sequence: string | null) => {
		if (sequence == null) {
			return;
		}

		const sent = controller.sendSyntheticInput(sequence);
		if (!sent) {
			onInlineError("terminal is not active");
			onClearModifiers();
		}
	};

	const keepTerminalFocus = (event: PointerEvent<HTMLButtonElement>) => {
		event.preventDefault();
	};

	const controls = [
		{
			label: "Ctrl",
			active: modifiers.has("ctrl"),
			pressed: modifiers.has("ctrl"),
			onClick: () => onToggleModifier("ctrl"),
		},
		{
			label: "Alt",
			active: modifiers.has("alt"),
			pressed: modifiers.has("alt"),
			onClick: () => onToggleModifier("alt"),
		},
		{
			label: "Shift",
			active: modifiers.has("shift"),
			pressed: modifiers.has("shift"),
			onClick: () => onToggleModifier("shift"),
		},
		{
			label: "Tab",
			onClick: () => send(sequenceForControlKey("tab", modifiers)),
		},
		{
			label: "Esc",
			onClick: () => send(sequenceForControlKey("esc", modifiers)),
		},
		{
			label: "End",
			onClick: () => send(sequenceForControlKey("end", modifiers)),
		},
		{
			label: "Enter",
			onClick: () => send(sequenceForControlKey("enter", modifiers)),
		},
		{
			label: "Bs",
			onClick: () => send(sequenceForControlKey("backspace", modifiers)),
		},
		{
			label: "Ctrl+C",
			onClick: () => send(sequenceForControlKey("ctrlc", modifiers)),
		},
		{
			label: "Ctrl+D",
			onClick: () => send(sequenceForControlKey("ctrld", modifiers)),
		},
		{
			label: "Ctrl+Z",
			onClick: () => send(sequenceForControlKey("ctrlz", modifiers)),
		},
		{
			label: "Ctrl+Y",
			onClick: () => send(sequenceForControlKey("ctrly", modifiers)),
		},
		{
			label: "↑",
			onClick: () => send(sequenceForControlKey("up", modifiers)),
		},
		{
			label: "↓",
			onClick: () => send(sequenceForControlKey("down", modifiers)),
		},
		{
			label: "←",
			onClick: () => send(sequenceForControlKey("left", modifiers)),
		},
		{
			label: "→",
			onClick: () => send(sequenceForControlKey("right", modifiers)),
		},
	];

	return (
		<section
			className="terminal-controls"
			aria-label="Mobile terminal controls"
		>
			<fieldset className="terminal-controls__grid">
				<legend className="sr-only">Terminal input keys</legend>
				{controls.map((control) => (
					<button
						key={control.label}
						type="button"
						className="terminal-controls__button"
						aria-label={control.label}
						data-active={control.active ? "true" : undefined}
						aria-pressed={control.pressed}
						onPointerDown={keepTerminalFocus}
						onClick={control.onClick}
					>
						{control.label}
					</button>
				))}
			</fieldset>
		</section>
	);
}
