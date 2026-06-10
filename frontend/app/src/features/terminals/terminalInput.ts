export type TerminalModifierKey = "ctrl" | "alt" | "shift";

export type TerminalControlKey =
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
	| "ctrld"
	| "ctrlz"
	| "ctrly";

const shiftedAscii: Record<string, string> = {
	"`": "~",
	"1": "!",
	"2": "@",
	"3": "#",
	"4": "$",
	"5": "%",
	"6": "^",
	"7": "&",
	"8": "*",
	"9": "(",
	"0": ")",
	"-": "_",
	"=": "+",
	"[": "{",
	"]": "}",
	"\\": "|",
	";": ":",
	"'": '"',
	",": "<",
	".": ">",
	"/": "?",
};
const lowercaseA = "a".charCodeAt(0);
const lowercaseZ = "z".charCodeAt(0);

export function sequenceForControlKey(
	key: TerminalControlKey,
	modifiers: ReadonlySet<TerminalModifierKey>,
) {
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
		case "ctrld":
			return "\u0004";
		case "ctrlz":
			return "\u001a";
		case "ctrly":
			return "\u0019";
	}
}

export function applyVirtualModifiersToInput(
	data: string,
	modifiers: ReadonlySet<TerminalModifierKey>,
) {
	if (data.length === 0 || modifiers.size === 0) {
		return data;
	}

	const controlKey = controlKeyForSequence(data);
	if (controlKey != null) {
		return sequenceForControlKey(controlKey, modifiers);
	}

	if (data.includes("\u001b")) {
		return data;
	}

	let output = "";
	for (const character of data) {
		output += applyModifiersToCharacter(character, modifiers);
	}
	return output;
}

function controlKeyForSequence(data: string): TerminalControlKey | null {
	switch (data) {
		case "\t":
			return "tab";
		case "\u001b":
			return "esc";
		case "\u001b[F":
			return "end";
		case "\u001b[A":
			return "up";
		case "\u001b[B":
			return "down";
		case "\u001b[D":
			return "left";
		case "\u001b[C":
			return "right";
		case "\r":
			return "enter";
		case "\u007f":
			return "backspace";
		default:
			return null;
	}
}

function applyModifiersToCharacter(
	character: string,
	modifiers: ReadonlySet<TerminalModifierKey>,
) {
	let modified = modifiers.has("shift")
		? applyShiftToCharacter(character)
		: character;

	if (modifiers.has("ctrl")) {
		modified = controlCharacterFor(modified);
	}

	if (modifiers.has("alt")) {
		modified = `\u001b${modified}`;
	}

	return modified;
}

function applyShiftToCharacter(character: string) {
	const codePoint = character.codePointAt(0);
	if (codePoint != null && codePoint >= lowercaseA && codePoint <= lowercaseZ) {
		return character.toUpperCase();
	}

	return shiftedAscii[character] ?? character;
}

function controlCharacterFor(character: string) {
	const codePoint = character.codePointAt(0);
	if (codePoint == null) {
		return character;
	}

	if (codePoint >= 0x41 && codePoint <= 0x5a) {
		return String.fromCharCode(codePoint - 0x40);
	}

	if (codePoint >= 0x61 && codePoint <= 0x7a) {
		return String.fromCharCode(codePoint - 0x60);
	}

	switch (character) {
		case " ":
		case "@":
		case "`":
			return "\u0000";
		case "[":
		case "{":
			return "\u001b";
		case "\\":
		case "|":
			return "\u001c";
		case "]":
		case "}":
			return "\u001d";
		case "^":
		case "~":
			return "\u001e";
		case "_":
			return "\u001f";
		case "?":
			return "\u007f";
		default:
			return character;
	}
}
