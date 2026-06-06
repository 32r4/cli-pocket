import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TerminalController } from "@/features/terminals/terminalController";
import type {
	TerminalRuntimeState,
	TerminalSessionRegistry,
} from "@/features/terminals/terminalSessionRegistry";
import type {
	SessionActor,
	TerminalInfoRecord,
	TerminalOpenAckRecord,
} from "@/platform/bridge/types";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { TerminalArea } from "./terminalArea";

afterEach(() => {
	cleanup();
});

function makeSession(): SessionActor {
	return {
		events: () => ({
			async *[Symbol.asyncIterator]() {},
		}),
		refreshTerminals: vi.fn(async () => undefined),
		openTerminal: vi.fn(
			async () =>
				({
					stream_id: 1,
					info: {
						terminal: "t1",
						cols: 80,
						rows: 24,
						created_at_unix_ms: 1,
						label: "shell",
						attached_clients: 1,
					},
					start_seq: 0,
					end_seq: 0,
					render_bytes_b64: "",
					has_more_history: false,
				}) satisfies TerminalOpenAckRecord,
		),
		readHistory: vi.fn(async () => ({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 0,
			bytes_b64: "",
			has_more: false,
		})),
		createTerminal: vi.fn(async () => null),
		getServerConfig: vi.fn(async () => ({ scrollback_bytes: 4 * 1024 * 1024 })),
		setServerConfig: vi.fn(async (config) => config),
		sendInput: vi.fn(async () => undefined),
		resize: vi.fn(async () => undefined),
		kill: vi.fn(async () => undefined),
		close: vi.fn(async () => undefined),
	};
}

function makeController(): TerminalController {
	return {
		setTheme: vi.fn(),
		removeTerminal: vi.fn(),
	} as unknown as TerminalController;
}

function makeRegistry(): TerminalSessionRegistry {
	return {
		connect: vi.fn(),
		activeRuntimeState: vi.fn(() => null),
		applyOutput: vi.fn(),
		disconnect: vi.fn(),
		removeTerminal: vi.fn(),
		retryActive: vi.fn(),
		setSelectedTerminal: vi.fn(),
		dispose: vi.fn(),
		mountActive: vi.fn(async () => undefined),
		unmountActive: vi.fn(),
		resizeActive: vi.fn(),
	} as unknown as TerminalSessionRegistry;
}

const terminalOne: TerminalInfoRecord = {
	terminal: "t1",
	cols: 80,
	rows: 24,
	created_at_unix_ms: 1,
	label: "old shell",
	attached_clients: 1,
};

const terminalTwo: TerminalInfoRecord = {
	terminal: "t2",
	cols: 100,
	rows: 30,
	created_at_unix_ms: 2,
	label: "new shell",
	attached_clients: 1,
};

describe("TerminalArea", () => {
	it("mounts and unmounts the active viewport through the registry", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const registry = makeRegistry();

		const view = render(
			<TerminalArea
				session={makeSession()}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(registry.mountActive).toHaveBeenCalledWith(
				expect.any(HTMLElement),
			);
		});

		view.unmount();

		expect(registry.unmountActive).toHaveBeenCalledTimes(1);
	});

	it("activates the newly created terminal when adding one", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const session = makeSession();
		session.createTerminal = vi.fn(async () => {
			workspaceState.getState().syncTerminalList([terminalOne, terminalTwo]);
			return terminalTwo;
		});
		const registry = makeRegistry();

		const view = render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		fireEvent.click(view.getByLabelText("Create terminal"));

		await waitFor(() => {
			expect(workspaceState.getState().activeSessionId).toBe("t2");
		});

		view.rerender(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(session.createTerminal).toHaveBeenCalledWith({
				cols: 120,
				rows: 36,
			});
		});
	});

	it("optimistically removes a killed terminal", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const session = makeSession();
		const controller = makeController();
		const registry = makeRegistry();

		const view = render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={controller}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		fireEvent.click(view.getByLabelText("Kill old shell"));

		await waitFor(() => {
			expect(session.kill).toHaveBeenCalledWith("t1", "TERM");
		});
		expect(workspaceState.getState().terminals).toHaveLength(0);
		expect(registry.removeTerminal).toHaveBeenCalledWith("t1");
		expect(controller.removeTerminal).toHaveBeenCalledWith("t1");
		expect(session.refreshTerminals).not.toHaveBeenCalled();
	});

	it("restores an optimistically removed terminal when kill fails", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const session = makeSession();
		session.kill = vi.fn(async () => {
			throw new Error("kill failed");
		});
		session.refreshTerminals = vi.fn(async () => {
			workspaceState.getState().syncTerminalList([terminalOne]);
		});
		const onInlineError = vi.fn();

		const view = render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={makeRegistry()}
				theme="dark"
				onInlineError={onInlineError}
			/>,
		);

		fireEvent.click(view.getByLabelText("Kill old shell"));

		await waitFor(() => {
			expect(workspaceState.getState().terminals).toHaveLength(1);
		});
		expect(session.refreshTerminals).toHaveBeenCalledTimes(1);
		expect(onInlineError).toHaveBeenCalledWith("kill failed");
	});

	it("shows retry when the active terminal open times out", () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const registry = makeRegistry();
		registry.activeRuntimeState = vi.fn<() => TerminalRuntimeState | null>(
			() => ({
				phase: "failed",
				error: "terminal open timed out",
			}),
		);

		const view = render(
			<TerminalArea
				session={makeSession()}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		fireEvent.click(view.getByRole("button", { name: "Retry" }));
		expect(registry.retryActive).toHaveBeenCalledTimes(1);
	});

	it("keeps the viewport mounted while opening and failed overlays are shown", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const registry = makeRegistry();
		const activeRuntimeState = vi
			.fn<() => TerminalRuntimeState | null>()
			.mockReturnValueOnce({
				phase: "opening",
				error: null,
			})
			.mockReturnValueOnce({
				phase: "failed",
				error: "terminal open timed out",
			});
		registry.activeRuntimeState = activeRuntimeState;

		const view = render(
			<TerminalArea
				session={makeSession()}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(registry.mountActive).toHaveBeenCalledTimes(1);
		});
		expect(registry.unmountActive).not.toHaveBeenCalled();

		view.rerender(
			<TerminalArea
				session={makeSession()}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		expect(registry.mountActive).toHaveBeenCalledTimes(1);
		expect(registry.unmountActive).not.toHaveBeenCalled();
	});
});
