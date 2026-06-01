import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type {
	SessionActor,
	TerminalInfoRecord,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { openTerminalSnapshot, TerminalArea } from "./terminalArea";

function makeSession(openTerminal: SessionActor["openTerminal"]): SessionActor {
	return {
		events: () => ({
			async *[Symbol.asyncIterator]() {},
		}),
		refreshTerminals: vi.fn(async () => undefined),
		openTerminal,
		createTerminal: vi.fn(async () => null),
		sendInput: vi.fn(async () => undefined),
		resize: vi.fn(async () => undefined),
		kill: vi.fn(async () => undefined),
		close: vi.fn(async () => undefined),
	};
}

describe("openTerminalSnapshot", () => {
	it("marks ready and renders decoded snapshot", async () => {
		const session = makeSession(
			vi.fn(
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
						snapshot_bytes_b64: btoa("hello"),
					}) satisfies TerminalSnapshotRecord,
			),
		);
		const onMarkTerminalConnecting = vi.fn();
		const onMarkTerminalReady = vi.fn();
		const onMarkTerminalError = vi.fn();
		const onInlineError = vi.fn();
		const onRenderSnapshot = vi.fn();

		await openTerminalSnapshot({
			session,
			terminalId: "t1",
			onMarkTerminalConnecting,
			onMarkTerminalReady,
			onMarkTerminalError,
			onInlineError,
			onRenderSnapshot,
		});

		expect(onMarkTerminalConnecting).toHaveBeenCalledWith("t1");
		expect(onMarkTerminalReady).toHaveBeenCalledWith({
			info: {
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "shell",
				attached_clients: 1,
			},
			snapshot_bytes_b64: btoa("hello"),
		});
		expect(onRenderSnapshot).toHaveBeenCalledWith("t1", "hello");
		expect(onMarkTerminalError).not.toHaveBeenCalled();
		expect(onInlineError).not.toHaveBeenCalled();
	});

	it("marks error when snapshot payload is invalid", async () => {
		const session = makeSession(vi.fn(async () => ({ bad: true }) as never));
		const onMarkTerminalConnecting = vi.fn();
		const onMarkTerminalReady = vi.fn();
		const onMarkTerminalError = vi.fn();
		const onInlineError = vi.fn();
		const onRenderSnapshot = vi.fn();

		await openTerminalSnapshot({
			session,
			terminalId: "t1",
			onMarkTerminalConnecting,
			onMarkTerminalReady,
			onMarkTerminalError,
			onInlineError,
			onRenderSnapshot,
		});

		expect(onMarkTerminalConnecting).toHaveBeenCalledWith("t1");
		expect(onMarkTerminalReady).not.toHaveBeenCalled();
		expect(onRenderSnapshot).not.toHaveBeenCalled();
		expect(onMarkTerminalError).toHaveBeenCalledWith(
			"t1",
			"invalid terminal snapshot",
		);
		expect(onInlineError).toHaveBeenCalledWith("invalid terminal snapshot");
	});
});

describe("TerminalArea", () => {
	it("opens the newly active terminal when activeSessionId changes externally", async () => {
		const session = makeSession(
			vi.fn(
				async () =>
					({
						info: {
							terminal: "t2",
							cols: 100,
							rows: 30,
							created_at_unix_ms: 2,
							label: "new shell",
							attached_clients: 1,
						},
						snapshot_bytes_b64: btoa("snapshot"),
					}) satisfies TerminalSnapshotRecord,
			),
		);
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "old shell",
				attached_clients: 1,
			},
			{
				terminal: "t2",
				cols: 100,
				rows: 30,
				created_at_unix_ms: 2,
				label: "new shell",
				attached_clients: 1,
			},
		]);
		workspaceState.getState().setActiveSessionId("t2");

		const controller = {
			setTheme: vi.fn(),
			setActiveTerminal: vi.fn(),
			setHandlers: vi.fn(),
			renderSnapshot: vi.fn(),
			mount: vi.fn(async () => undefined),
			unmount: vi.fn(),
		};

		render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={controller as never}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(session.openTerminal).toHaveBeenCalledWith("t2");
			expect(controller.renderSnapshot).toHaveBeenCalledWith("t2", "snapshot");
		});
	});

	it("activates the newly created terminal when adding one", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "old shell",
				attached_clients: 1,
			},
		]);
		workspaceState.getState().setActiveSessionId("t1");

		const createdTerminal: TerminalInfoRecord = {
			terminal: "t2",
			cols: 120,
			rows: 36,
			created_at_unix_ms: 2,
			label: "new shell",
			attached_clients: 1,
		};
		const session = makeSession(
			vi.fn(
				async () =>
					({
						info: createdTerminal,
						snapshot_bytes_b64: btoa("snapshot"),
					}) satisfies TerminalSnapshotRecord,
			),
		);
		session.createTerminal = vi.fn(async () => {
			workspaceState.getState().syncTerminalList([
				{
					terminal: "t1",
					cols: 80,
					rows: 24,
					created_at_unix_ms: 1,
					label: "old shell",
					attached_clients: 1,
				},
				createdTerminal,
			]);
			return createdTerminal;
		});

		const controller = {
			setTheme: vi.fn(),
			setActiveTerminal: vi.fn(),
			setHandlers: vi.fn(),
			renderSnapshot: vi.fn(),
			mount: vi.fn(async () => undefined),
			unmount: vi.fn(),
		};

		const view = render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={controller as never}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		const addButton = view.container.querySelector(".terminal-tab--add");
		if (addButton == null) {
			throw new Error("expected add button");
		}
		fireEvent.click(addButton);

		await waitFor(() => {
			expect(workspaceState.getState().activeSessionId).toBe("t2");
		});

		view.rerender(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={controller as never}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(session.createTerminal).toHaveBeenCalledWith({
				cols: 120,
				rows: 36,
			});
			expect(workspaceState.getState().activeSessionId).toBe("t2");
			expect(session.openTerminal).toHaveBeenCalledWith("t2");
		});
	});

	it("refreshes the terminal list after killing one", async () => {
		const workspaceState = createWorkspaceStore();
		workspaceState.getState().markConnected();
		workspaceState.getState().syncTerminalList([
			{
				terminal: "t1",
				cols: 80,
				rows: 24,
				created_at_unix_ms: 1,
				label: "shell",
				attached_clients: 1,
			},
		]);
		workspaceState.getState().setActiveSessionId("t1");

		const session = makeSession(
			vi.fn(
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
						snapshot_bytes_b64: btoa("snapshot"),
					}) satisfies TerminalSnapshotRecord,
			),
		);
		session.refreshTerminals = vi.fn(async () => undefined);

		const controller = {
			setTheme: vi.fn(),
			setActiveTerminal: vi.fn(),
			setHandlers: vi.fn(),
			renderSnapshot: vi.fn(),
			mount: vi.fn(async () => undefined),
			unmount: vi.fn(),
		};

		const view = render(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={controller as never}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);

		const killButton = view.getByLabelText("Kill shell");
		fireEvent.click(killButton);
		view.rerender(
			<TerminalArea
				session={session}
				workspace={workspaceState.getState()}
				workspaceState={workspaceState}
				controller={controller as never}
				theme="dark"
				onInlineError={vi.fn()}
			/>,
		);
		expect(view.queryByLabelText("Kill shell")).toBeNull();

		await waitFor(() => {
			expect(session.kill).toHaveBeenCalledWith("t1", "TERM");
			expect(workspaceState.getState().terminals).toHaveLength(0);
		});
	});
});
