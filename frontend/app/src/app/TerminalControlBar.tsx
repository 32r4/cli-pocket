import { useState } from "react";
import type { TerminalController } from "@/features/terminals/terminalController";

type ModifierKey = "ctrl" | "alt" | "shift";
type ControlKey =
	| "tab"
	| "esc"
	| "end"
	| "up"
	| "down"
	| "left"
	| "right"
	| "enter"
	| "backspace"
	| "ctrlc"
	| "ctrlv"
	| "ctrlz"
	| "ctrly";

interface TerminalControlBarProps {
	controller: TerminalController;
	onInlineError: (message: string | null) => void;
}

function sequenceForKey(key: ControlKey, modifiers: Set<ModifierKey>) {
	const ctrl = modifiers.has("ctrl");
	const alt = modifiers.has("alt");
	const shift = modifiers.has("shift");
	const anyModifier = ctrl || alt || shift;
	const modifierParameter =
		Number(shift) + Number(alt) * 2 + Number(ctrl) * 4 + 1;

	switch (key) {
		case "tab":
			if (shift && !ctrl && !alt) {
				return "\u001b[Z";
			}
			if (anyModifier) {
				return `\u001b[1;${modifierParameter}I`;
			}
			return "\t";
		case "esc":
			return alt ? "\u001b\u001b" : "\u001b";
		case "end":
			return anyModifier ? `\u001b[1;${modifierParameter}F` : "\u001b[F";
		case "up":
			return anyModifier ? `\u001b[1;${modifierParameter}A` : "\u001b[A";
		case "down":
			return anyModifier ? `\u001b[1;${modifierParameter}B` : "\u001b[B";
		case "left":
			return anyModifier ? `\u001b[1;${modifierParameter}D` : "\u001b[D";
		case "right":
			return anyModifier ? `\u001b[1;${modifierParameter}C` : "\u001b[C";
		case "enter":
			return alt ? "\u001b\r" : "\r";
		case "backspace":
			return alt ? "\u001b\u007f" : "\u007f";
		case "ctrlc":
			return "\u0003";
		case "ctrlv":
			return null;
		case "ctrlz":
			return "\u001a";
		case "ctrly":
			return "\u0019";
	}
}

export function TerminalControlBar({
	controller,
	onInlineError,
}: TerminalControlBarProps) {
	const [latchedModifiers, setLatchedModifiers] = useState<Set<ModifierKey>>(
		() => new Set(),
	);

	const send = (sequence: string | null) => {
		if (sequence == null) {
			return;
		}

		const sent = controller.sendSyntheticInput(sequence);
		if (!sent) {
			onInlineError("terminal is not active");
			setLatchedModifiers(new Set());
		}
	};

	const handlePaste = async () => {
		try {
			if (typeof navigator === "undefined" || navigator.clipboard == null) {
				throw new Error("clipboard unavailable");
			}
			const text = await navigator.clipboard.readText();
			if (text.length === 0) {
				return;
			}
			const pasted = controller.pasteText(text);
			if (!pasted) {
				onInlineError("terminal is not active");
				setLatchedModifiers(new Set());
			}
		} catch (error: unknown) {
			onInlineError(
				error instanceof Error ? error.message : "failed to paste text",
			);
		}
	};

	const toggleModifier = (modifier: ModifierKey) => {
		setLatchedModifiers((current) => {
			const next = new Set(current);
			if (next.has(modifier)) {
				next.delete(modifier);
			} else {
				next.add(modifier);
			}
			return next;
		});
	};

	const controls = [
		{
			label: "Ctrl",
			active: latchedModifiers.has("ctrl"),
			pressed: latchedModifiers.has("ctrl"),
			onClick: () => toggleModifier("ctrl"),
		},
		{
			label: "Alt",
			active: latchedModifiers.has("alt"),
			pressed: latchedModifiers.has("alt"),
			onClick: () => toggleModifier("alt"),
		},
		{
			label: "Shift",
			active: latchedModifiers.has("shift"),
			pressed: latchedModifiers.has("shift"),
			onClick: () => toggleModifier("shift"),
		},
		{
			label: "Tab",
			onClick: () => send(sequenceForKey("tab", latchedModifiers)),
		},
		{
			label: "Esc",
			onClick: () => send(sequenceForKey("esc", latchedModifiers)),
		},
		{
			label: "End",
			onClick: () => send(sequenceForKey("end", latchedModifiers)),
		},
		{
			label: "Enter",
			onClick: () => send(sequenceForKey("enter", latchedModifiers)),
		},
		{
			label: "Bs",
			onClick: () => send(sequenceForKey("backspace", latchedModifiers)),
		},
		{
			label: "Ctrl+C",
			onClick: () => send(sequenceForKey("ctrlc", latchedModifiers)),
		},
		{
			label: "Ctrl+V",
			onClick: () => void handlePaste(),
		},
		{
			label: "Ctrl+Z",
			onClick: () => send(sequenceForKey("ctrlz", latchedModifiers)),
		},
		{
			label: "Ctrl+Y",
			onClick: () => send(sequenceForKey("ctrly", latchedModifiers)),
		},
		{
			label: "↑",
			onClick: () => send(sequenceForKey("up", latchedModifiers)),
		},
		{
			label: "↓",
			onClick: () => send(sequenceForKey("down", latchedModifiers)),
		},
		{
			label: "←",
			onClick: () => send(sequenceForKey("left", latchedModifiers)),
		},
		{
			label: "→",
			onClick: () => send(sequenceForKey("right", latchedModifiers)),
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
						onClick={control.onClick}
					>
						{control.label}
					</button>
				))}
			</fieldset>
		</section>
	);
}
