import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
	SessionActor,
	TerminalInfoRecord,
	TerminalSnapshotRecord,
} from "@/platform/bridge/types";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { openTerminalSnapshot, TerminalArea } from "./terminalArea";

afterEach(() => {
	cleanup();
});

function makeSession(openTerminal: SessionActor["openTerminal"]): SessionActor {
	return {
		events: () => ({
			async *[Symbol.asyncIterator]() {},
		}),
		refreshTerminals: vi.fn(async () => undefined),
		openTerminal,
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

describe("openTerminalSnapshot", () => {
	it("does not mark terminal connecting before a session exists", async () => {
		const onMarkTerminalConnecting = vi.fn();
		const onMarkTerminalReady = vi.fn();
		const onMarkTerminalError = vi.fn();
		const onInlineError = vi.fn();
		const onRenderSnapshot = vi.fn();

		await openTerminalSnapshot({
			session: null,
			terminalId: "t1",
			onMarkTerminalConnecting,
			onMarkTerminalReady,
			onMarkTerminalError,
			onInlineError,
			onRenderSnapshot,
		});

		expect(onMarkTerminalConnecting).not.toHaveBeenCalled();
		expect(onMarkTerminalReady).not.toHaveBeenCalled();
		expect(onMarkTerminalError).not.toHaveBeenCalled();
		expect(onRenderSnapshot).not.toHaveBeenCalled();
		expect(onInlineError).not.toHaveBeenCalled();
	});

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
						start_seq: 0,
						end_seq: 5,
						render_prefix_b64: btoa(""),
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
			start_seq: 0,
			end_seq: 5,
			render_prefix_b64: btoa(""),
			snapshot_bytes_b64: btoa("hello"),
		});
		expect(onRenderSnapshot).toHaveBeenCalledWith("t1", "hello", 0);
		expect(onMarkTerminalError).not.toHaveBeenCalled();
		expect(onInlineError).not.toHaveBeenCalled();
	});

	it("preloads shared history before rendering the initial terminal window", async () => {
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
						start_seq: 5,
						end_seq: 10,
						render_prefix_b64: btoa(""),
						snapshot_bytes_b64: btoa("world"),
					}) satisfies TerminalSnapshotRecord,
			),
		);
		session.readHistory = vi.fn(async () => ({
			terminal_id: "t1",
			start_seq: 0,
			end_seq: 5,
			bytes_b64: btoa("hello"),
		}));
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

		expect(session.readHistory).toHaveBeenCalledWith("t1", 5, 32 * 1024);
		expect(onRenderSnapshot).toHaveBeenCalledWith("t1", "helloworld", 0);
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
						start_seq: 0,
						end_seq: 8,
						render_prefix_b64: btoa(""),
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
			renderSnapshotWithRange: vi.fn(),
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
			expect(controller.renderSnapshotWithRange).toHaveBeenCalledWith(
				"t2",
				"snapshot",
				0,
			);
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
						start_seq: 0,
						end_seq: 8,
						render_prefix_b64: btoa(""),
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
			renderSnapshotWithRange: vi.fn(),
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

		const addButton = view.getByLabelText("Create terminal");
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
						start_seq: 0,
						end_seq: 8,
						render_prefix_b64: btoa(""),
						snapshot_bytes_b64: btoa("snapshot"),
					}) satisfies TerminalSnapshotRecord,
			),
		);
		session.refreshTerminals = vi.fn(async () => undefined);

		const controller = {
			setTheme: vi.fn(),
			setActiveTerminal: vi.fn(),
			setHandlers: vi.fn(),
			renderSnapshotWithRange: vi.fn(),
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
