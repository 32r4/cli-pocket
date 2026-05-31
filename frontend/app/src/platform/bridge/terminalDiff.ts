import type { TerminalInfoRecord } from "./types";

export function findCreatedTerminal(
	before: TerminalInfoRecord[],
	after: TerminalInfoRecord[],
) {
	const beforeIds = new Set(before.map((terminal) => terminal.terminal));
	return after.find((terminal) => !beforeIds.has(terminal.terminal)) ?? null;
}
