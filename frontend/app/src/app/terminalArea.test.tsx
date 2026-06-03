import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TerminalController } from "@/features/terminals/terminalController";
import type { TerminalSessionRegistry } from "@/features/terminals/terminalSessionRegistry";
import type {
	SessionActor,
	TerminalInfoRecord,
	TerminalSnapshotRecord,
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
		activateTerminal: vi.fn(
			async () =>
				({
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
					render_prefix_b64: "",
					snapshot_bytes_b64: "",
				}) satisfies TerminalSnapshotRecord,
		),
		readHistory: vi.fn(async () => ({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 0,
			bytes_b64: "",
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
	} as unknown as TerminalController;
}

function makeRegistry(): TerminalSessionRegistry {
	return {
		activateTerminal: vi.fn(),
		applyOutput: vi.fn(),
		disconnect: vi.fn(),
		removeTerminal: vi.fn(),
		setActiveTerminalId: vi.fn(),
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
	it("activates the selected terminal through the registry", async () => {
		const session = makeSession();
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne, terminalTwo]);
		workspaceState.getState().setActiveSessionId("t2");
		const registry = makeRegistry();

		render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				connectionGeneration={7}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(registry.activateTerminal).toHaveBeenCalledWith("t2", 7);
		});
		expect(session.activateTerminal).not.toHaveBeenCalled();
	});

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
				connectionGeneration={1}
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
				connectionGeneration={3}
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
				connectionGeneration={3}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(session.createTerminal).toHaveBeenCalledWith({
				cols: 120,
				rows: 36,
			});
			expect(registry.activateTerminal).toHaveBeenCalledWith("t2", 3);
		});
	});

	it("removes the killed terminal from workspace and registry", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([terminalOne]);
		workspaceState.getState().setActiveSessionId("t1");
		const session = makeSession();
		const registry = makeRegistry();

		const view = render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				connectionGeneration={1}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		fireEvent.click(view.getByLabelText("Kill old shell"));
		view.rerender(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={makeController()}
				registry={registry}
				connectionGeneration={1}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		expect(view.queryByLabelText("Kill old shell")).toBeNull();
		expect(registry.removeTerminal).toHaveBeenCalledWith("t1");
		await waitFor(() => {
			expect(session.kill).toHaveBeenCalledWith("t1", "TERM");
			expect(workspaceState.getState().terminals).toHaveLength(0);
		});
	});
});
