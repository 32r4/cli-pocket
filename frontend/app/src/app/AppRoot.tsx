import { useEffect, useMemo, useState } from "react";
import { useStore } from "zustand";
import { pairAndStoreDaemon } from "@/features/pairing/pairingFlow";
import { importPairingOfferUrl } from "@/features/pairing/pairingOffer";
import { SessionController } from "@/features/terminals/SessionController";
import { XTermView } from "@/features/terminals/XTermView";
import type { ClientBridge, ConnectConfig } from "@/platform/bridge/types";
import { TauriBridge } from "@/platform/tauri/TauriBridge";
import { WebBridge } from "@/platform/web/WebBridge";
import { ErrorBanner } from "@/shared/components/ErrorBanner";
import { createDaemonRegistryStore } from "@/state/daemon-registry/daemonRegistry";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { createUiStateStore, type OverlaySection } from "@/state/ui/uiState";
import { createWorkspaceStore } from "@/state/workspace/workspaceState";
import { Shell } from "./shell/Shell";

const daemonRegistry = createDaemonRegistryStore();
const uiState = createUiStateStore();
const workspaceState = createWorkspaceStore();

interface AppRootProps {
	clientKind: "web" | "tauri";
	mobile?: boolean;
}

type DetailMode = "view" | "add" | "edit";

interface HostFormState {
	kind: "direct" | "relay";
	endpointUrl: string;
	relayUrl: string;
	hostId: string;
	relayPskHex: string;
	serverPublicHex: string;
}

function toConnectConfig(host: DaemonRecord): ConnectConfig {
	if (host.kind === "direct") {
		return {
			kind: "direct",
			endpointUrl: host.endpointUrl,
			resumeTokenHex: host.resumeTokenHex ?? undefined,
		};
	}

	return {
		kind: "relay",
		relayUrl: host.relayUrl,
		hostId: host.hostId,
		pskHex: host.relayPskHex,
		serverPublicHex: host.serverPublicHex,
		resumeTokenHex: host.resumeTokenHex ?? undefined,
	};
}

function hostBadge(host: DaemonRecord) {
	return host.kind === "direct" ? "Local" : "Remote";
}

function endpointLabel(host: DaemonRecord) {
	return host.kind === "direct" ? host.endpointUrl : host.relayUrl;
}

function initialFormState(host?: DaemonRecord): HostFormState {
	if (host == null) {
		return {
			kind: "direct",
			endpointUrl: "ws://127.0.0.1:7842/session",
			relayUrl: "wss://relay.example/ws/client?host=",
			hostId: "",
			relayPskHex: "",
			serverPublicHex: "",
		};
	}

	if (host.kind === "direct") {
		return {
			kind: "direct",
			endpointUrl: host.endpointUrl,
			relayUrl: "",
			hostId: host.id,
			relayPskHex: "",
			serverPublicHex: "",
		};
	}

	return {
		kind: "relay",
		endpointUrl: "",
		relayUrl: host.relayUrl,
		hostId: host.hostId,
		relayPskHex: host.relayPskHex,
		serverPublicHex: host.serverPublicHex,
	};
}

function makeHostRecord(
	form: HostFormState,
	currentHostId: string | null,
): DaemonRecord {
	if (form.kind === "direct") {
		const id = currentHostId ?? crypto.randomUUID();
		return {
			id,
			label: currentHostId ?? id,
			kind: "direct",
			endpointUrl: form.endpointUrl.trim(),
			resumeTokenHex: null,
			lastConnectedAt: null,
		};
	}

	const hostId = form.hostId.trim() || crypto.randomUUID();
	const serverPublicHex = form.serverPublicHex.trim() || "00".repeat(32);
	return {
		id: currentHostId ?? hostId,
		label: currentHostId ?? hostId,
		kind: "relay",
		hostId,
		relayUrl: form.relayUrl.trim(),
		relayPskHex: form.relayPskHex.trim() || "00".repeat(32),
		serverPublicHex,
		resumeTokenHex: null,
		lastConnectedAt: null,
	};
}

async function createBridge(clientKind: "web" | "tauri") {
	if (clientKind === "tauri") {
		return new TauriBridge();
	}

	return WebBridge.create();
}

export function AppRoot({ clientKind, mobile = false }: AppRootProps) {
	const registry = useStore(daemonRegistry);
	const ui = useStore(uiState);
	const workspace = useStore(workspaceState);

	const [bridge, setBridge] = useState<ClientBridge | null>(null);
	const [bridgeError, setBridgeError] = useState<string | null>(null);
	const [controller, setController] = useState<SessionController | null>(null);
	const [detailMode, setDetailMode] = useState<DetailMode>("view");
	const [hostForm, setHostForm] = useState<HostFormState>(() =>
		initialFormState(),
	);
	const [pairingUrl, setPairingUrl] = useState("");
	const [inlineError, setInlineError] = useState<string | null>(null);

	const selectedHost =
		registry.daemons.find((daemon) => daemon.id === ui.selectedHostId) ?? null;
	const activeHost =
		registry.daemons.find(
			(daemon) => daemon.id === workspace.activeConnectionHostId,
		) ?? null;
	const mainHost = selectedHost ?? activeHost ?? registry.daemons[0] ?? null;
	const activeSession =
		workspace.terminals.find(
			(terminal) => terminal.id === workspace.activeSessionId,
		) ??
		workspace.terminals[0] ??
		null;

	useEffect(() => {
		let active = true;

		void createBridge(clientKind)
			.then((instance) => {
				if (!active) {
					void instance.close();
					return;
				}

				setBridge(instance);
				setController(new SessionController(instance, workspaceState));
			})
			.catch((error: unknown) => {
				if (!active) {
					return;
				}
				setBridgeError(
					error instanceof Error
						? error.message
						: "failed to start client bridge",
				);
			});

		return () => {
			active = false;
		};
	}, [clientKind]);

	useEffect(() => {
		const fallbackHostId = registry.daemons[0]?.id ?? null;
		const selectedHostStillExists = registry.daemons.some(
			(daemon) => daemon.id === ui.selectedHostId,
		);

		if (!selectedHostStillExists && ui.selectedHostId !== fallbackHostId) {
			uiState.getState().setSelectedHostId(fallbackHostId);
		}

		if (registry.selectedDaemonId == null && fallbackHostId != null) {
			registry.selectDaemon(fallbackHostId);
		}
	}, [
		registry.daemons,
		registry.selectedDaemonId,
		ui.selectedHostId,
		registry,
	]);

	useEffect(() => {
		if (selectedHost == null) {
			return;
		}

		if (detailMode === "view") {
			setHostForm(initialFormState(selectedHost));
		}
	}, [detailMode, selectedHost]);

	useEffect(() => {
		if (
			bridge == null ||
			workspace.activeConnectionHostId == null ||
			workspace.connectionState !== "connected"
		) {
			return;
		}

		let cancelled = false;

		void (async () => {
			try {
				for await (const event of bridge.events()) {
					if (cancelled || typeof event !== "object" || event === null) {
						if (cancelled) {
							break;
						}
						continue;
					}

					const kind =
						"kind" in event && typeof event.kind === "string"
							? event.kind
							: null;
					if (kind === "Connecting") {
						continue;
					}
					if (kind === "Connected") {
						const hostLabel =
							"host_label" in event && typeof event.host_label === "string"
								? event.host_label.trim()
								: "";
						if (
							hostLabel.length > 0 &&
							workspaceState.getState().activeConnectionHostId != null
						) {
							daemonRegistry
								.getState()
								.updateDaemonLabel(
									workspaceState.getState().activeConnectionHostId as string,
									hostLabel,
								);
						}
						workspaceState.getState().markConnected();
						continue;
					}
					if (kind === "Disconnected") {
						workspaceState.getState().markDisconnected();
						continue;
					}
					if (kind === "TerminalCreated") {
						const info =
							"info" in event &&
							typeof event.info === "object" &&
							event.info !== null
								? event.info
								: null;
						const terminalId =
							info != null &&
							"terminal" in info &&
							typeof info.terminal === "string"
								? info.terminal
								: `terminal-${workspaceState.getState().terminals.length + 1}`;
						const label =
							info != null && "label" in info && typeof info.label === "string"
								? info.label
								: `Terminal ${workspaceState.getState().terminals.length + 1}`;

						workspaceState.getState().openTerminal({
							id: terminalId,
							title: label,
							status: "ready",
						});
						continue;
					}
					if (kind === "TerminalExited") {
						const terminalId =
							"terminal_id" in event && typeof event.terminal_id === "string"
								? event.terminal_id
								: null;
						if (terminalId !== null) {
							workspaceState.getState().markTerminalClosed(terminalId);
						}
						continue;
					}
					if (kind === "Error") {
						const message =
							"message" in event && typeof event.message === "string"
								? event.message
								: "runtime error";
						workspaceState.getState().markConnectionFailed(message);
						setInlineError(message);
					}
				}
			} catch (error: unknown) {
				if (cancelled) {
					return;
				}
				const message =
					error instanceof Error ? error.message : "event stream failed";
				workspaceState.getState().markConnectionFailed(message);
				setInlineError(message);
			}
		})();

		return () => {
			cancelled = true;
		};
	}, [bridge, workspace.activeConnectionHostId, workspace.connectionState]);

	const statusText = useMemo(() => {
		switch (workspace.connectionState) {
			case "connecting":
				return "connecting";
			case "connected":
				return "connected";
			case "failed":
				return "failed";
			default:
				return "idle";
		}
	}, [workspace.connectionState]);

	const connectSelectedHost = async () => {
		if (selectedHost == null || controller == null) {
			return;
		}

		setInlineError(null);
		try {
			registry.selectDaemon(selectedHost.id);
			await controller.connectAndCreate(
				selectedHost.id,
				toConnectConfig(selectedHost),
			);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "connection failed";
			workspaceState.getState().markConnectionFailed(message);
			setInlineError(message);
		}
	};

	const createSession = async () => {
		if (bridge == null || workspace.connectionState !== "connected") {
			return;
		}

		const nextIndex = workspace.terminals.length + 1;
		const tempId = `pending-terminal-${nextIndex}`;
		workspaceState.getState().openTerminal({
			id: tempId,
			title: `Terminal ${nextIndex}`,
			status: "connecting",
		});

		try {
			await bridge.createTerminal({ cols: 120, rows: 36 });
			workspaceState.getState().markTerminalReady(tempId);
		} catch (error: unknown) {
			const message =
				error instanceof Error ? error.message : "failed to create terminal";
			workspaceState.getState().markTerminalClosed(tempId);
			setInlineError(message);
		}
	};

	const disconnectActiveHost = () => {
		void bridge?.close();
		workspaceState.getState().markDisconnected();
	};

	const openHost = (hostId: string) => {
		uiState.getState().setSelectedHostId(hostId);
		uiState.getState().setOverlaySection("host");
		setDetailMode("view");
	};

	const startAddHost = () => {
		setDetailMode("add");
		setInlineError(null);
		setHostForm(initialFormState());
		uiState.getState().openOverlay("host");
		uiState.getState().setOverlaySection("host");
	};

	const startEditHost = () => {
		if (selectedHost == null) {
			return;
		}
		setDetailMode("edit");
		setInlineError(null);
		setHostForm(initialFormState(selectedHost));
	};

	const saveHost = () => {
		if (detailMode === "add" && hostForm.kind === "relay") {
			return;
		}

		const wasAdding = detailMode === "add";
		const currentHostId =
			detailMode === "edit" ? (selectedHost?.id ?? null) : null;
		const nextHost = makeHostRecord(hostForm, currentHostId);
		registry.upsertDaemon(nextHost);
		registry.selectDaemon(nextHost.id);
		uiState.getState().setSelectedHostId(nextHost.id);
		setDetailMode("view");
		if (wasAdding) {
			uiState.getState().closeOverlay();
		}
	};

	const removeHost = () => {
		if (selectedHost == null) {
			return;
		}

		if (workspace.activeConnectionHostId === selectedHost.id) {
			setInlineError("disconnect the active host before removing it");
			return;
		}

		registry.removeDaemon(selectedHost.id);
		const fallback = daemonRegistry.getState().daemons[0] ?? null;
		uiState.getState().setSelectedHostId(fallback?.id ?? null);
		setDetailMode("view");
	};

	const importPairingLink = async () => {
		try {
			const importedHost = importPairingOfferUrl(pairingUrl);
			await pairAndStoreDaemon(pairingUrl, registry.upsertDaemon);
			registry.selectDaemon(importedHost.id);
			uiState.getState().setSelectedHostId(importedHost.id);
			setPairingUrl("");
			setInlineError(null);
			setDetailMode("view");
		} catch (error: unknown) {
			setInlineError(
				error instanceof Error
					? error.message
					: "failed to import pairing link",
			);
		}
	};

	const hostForConnectionPanel =
		selectedHost ?? activeHost ?? registry.daemons[0] ?? null;
	const errorMessage = inlineError ?? workspace.lastError ?? bridgeError;
	const isRelayImportMode = detailMode === "add" && hostForm.kind === "relay";
	const isRelayEditMode =
		detailMode === "edit" &&
		selectedHost?.kind === "relay" &&
		hostForm.kind === "relay";

	return (
		<Shell
			clientKind={clientKind}
			statusText={statusText}
			activeHostLabel={activeHost?.label ?? null}
			activeEndpoint={activeHost ? endpointLabel(activeHost) : null}
			isOverlayOpen={ui.isOverlayOpen}
			onOpenOverlay={() => ui.openOverlay("host")}
			onCloseOverlay={ui.closeOverlay}
		>
			<main className="app-shell__main">
				{workspace.connectionState === "connected" ? (
					<section className="workspace-panel" aria-label="Terminal workspace">
						<div className="terminal-tabs" role="tablist" aria-label="Sessions">
							{workspace.terminals.map((terminal) => (
								<button
									className="terminal-tab"
									type="button"
									key={terminal.id}
									data-active={terminal.id === activeSession?.id}
									onClick={() =>
										workspaceState.getState().setActiveSessionId(terminal.id)
									}
								>
									{terminal.title}
								</button>
							))}
							<button
								className="terminal-tab terminal-tab--add"
								type="button"
								onClick={() => void createSession()}
							>
								+
							</button>
						</div>
						<div className="terminal-stage">
							<XTermView title={activeSession?.title ?? "Terminal"} />
						</div>
						<footer className="terminal-footer">
							<span>{activeSession?.title ?? "No terminal"}</span>
							<span>120x36</span>
							<span>{activeSession?.status ?? workspace.connectionState}</span>
						</footer>
					</section>
				) : (
					<section className="connect-panel" aria-label="Connect to a host">
						<h2>Connect to a host</h2>
						<div className="field-stack">
							<label className="field">
								<span>Selected host</span>
								<select
									value={mainHost?.id ?? ""}
									onChange={(event) => {
										registry.selectDaemon(event.target.value);
										uiState.getState().setSelectedHostId(event.target.value);
									}}
									disabled={registry.daemons.length === 0}
								>
									{registry.daemons.length === 0 ? (
										<option value="">No saved hosts</option>
									) : null}
									{registry.daemons.map((host) => (
										<option value={host.id} key={host.id}>
											{host.label}
										</option>
									))}
								</select>
							</label>
							<div className="field field--static">
								<span>Endpoint</span>
								<strong>
									{mainHost ? endpointLabel(mainHost) : "No saved hosts"}
								</strong>
							</div>
						</div>
						<div className="action-row">
							<button
								type="button"
								onClick={() => void connectSelectedHost()}
								disabled={mainHost == null || controller == null}
							>
								Connect
							</button>
							<button type="button" onClick={startAddHost}>
								Add host
							</button>
							<button
								type="button"
								onClick={() => uiState.getState().openOverlay("host")}
							>
								Open hosts and settings
							</button>
						</div>
						<div className="field">
							<span>Pair with link</span>
							<div className="inline-form">
								<input
									value={pairingUrl}
									onChange={(event) => setPairingUrl(event.target.value)}
									placeholder="https://cli-pocket...#pair=..."
								/>
								<button type="button" onClick={() => void importPairingLink()}>
									Import
								</button>
							</div>
						</div>
						<button type="button" className="ghost-button" disabled>
							Scan QR code
						</button>
					</section>
				)}

				<ErrorBanner message={errorMessage} />
			</main>

			{ui.isOverlayOpen ? (
				<aside className="control-overlay" aria-label="Control overlay">
					<div
						className="control-overlay__rail"
						data-mobile={mobile ? "1" : "0"}
					>
						<button
							type="button"
							className="back-button"
							onClick={ui.closeOverlay}
						>
							Back
						</button>
						<nav className="overlay-nav" aria-label="Overlay sections">
							{(
								["host", "settings", "diagnostics", "about"] as OverlaySection[]
							).map((section) => (
								<button
									type="button"
									key={section}
									data-active={ui.overlaySection === section}
									onClick={() => {
										uiState.getState().setOverlaySection(section);
										if (section !== "host") {
											setDetailMode("view");
										}
									}}
								>
									{section === "host"
										? "Connection"
										: section.charAt(0).toUpperCase() + section.slice(1)}
								</button>
							))}
						</nav>
						<div className="host-list">
							{registry.daemons.map((host) => (
								<button
									type="button"
									key={host.id}
									className="host-list__item"
									data-active={ui.selectedHostId === host.id}
									onClick={() => openHost(host.id)}
								>
									<span>{host.label}</span>
									<small>{hostBadge(host)}</small>
								</button>
							))}
							<button
								type="button"
								className="host-list__add"
								onClick={startAddHost}
							>
								+ Add host
							</button>
						</div>
					</div>
					<div className="control-overlay__detail">
						{ui.overlaySection === "host" ? (
							<section className="detail-section">
								<h2>
									{detailMode === "add"
										? "Add host"
										: detailMode === "edit"
											? "Edit host"
											: (hostForConnectionPanel?.label ?? "Connection")}
								</h2>
								{detailMode === "view" && hostForConnectionPanel != null ? (
									<>
										<div
											className="status-chip"
											data-state={workspace.connectionState}
										>
											{workspace.activeConnectionHostId ===
											hostForConnectionPanel.id
												? statusText
												: "saved"}
										</div>
										<div className="detail-grid">
											<div>
												<span>Status</span>
												<strong>
													{workspace.activeConnectionHostId ===
													hostForConnectionPanel.id
														? statusText
														: "not active"}
												</strong>
											</div>
											<div>
												<span>Endpoint</span>
												<strong>{endpointLabel(hostForConnectionPanel)}</strong>
											</div>
											<div>
												<span>Pairing</span>
												<strong>
													{hostForConnectionPanel.kind === "relay"
														? "paired"
														: "direct"}
												</strong>
											</div>
										</div>
										<div className="action-column">
											<button
												type="button"
												onClick={() => void connectSelectedHost()}
											>
												Connect
											</button>
											<button type="button" onClick={disconnectActiveHost}>
												Disconnect
											</button>
											<button type="button" onClick={startEditHost}>
												Edit host
											</button>
											<button type="button" onClick={removeHost}>
												Remove host
											</button>
										</div>
									</>
								) : detailMode === "view" ? (
									<div className="empty-state">
										<p>No saved hosts yet.</p>
										<button type="button" onClick={startAddHost}>
											Add host
										</button>
									</div>
								) : (
									<form
										className="host-form"
										onSubmit={(event) => {
											event.preventDefault();
											saveHost();
										}}
									>
										{detailMode === "add" ? (
											<label className="field">
												<span>Mode</span>
												<select
													value={hostForm.kind}
													onChange={(event) =>
														setHostForm((state) => ({
															...state,
															kind: event.target.value as "direct" | "relay",
														}))
													}
												>
													<option value="direct">Direct</option>
													<option value="relay">Relay</option>
												</select>
											</label>
										) : (
											<div className="field field--static">
												<span>Mode</span>
												<strong>
													{hostForm.kind === "direct" ? "Direct" : "Relay"}
												</strong>
											</div>
										)}
										{hostForm.kind === "direct" ? (
											<label className="field">
												<span>Endpoint URL</span>
												<input
													value={hostForm.endpointUrl}
													onChange={(event) =>
														setHostForm((state) => ({
															...state,
															endpointUrl: event.target.value,
														}))
													}
												/>
											</label>
										) : isRelayImportMode ? (
											<label className="field">
												<span>Pairing link</span>
												<div className="inline-form">
													<input
														value={pairingUrl}
														onChange={(event) =>
															setPairingUrl(event.target.value)
														}
														placeholder="https://cli-pocket...#pair=..."
													/>
													<button
														type="button"
														onClick={() => void importPairingLink()}
													>
														Import
													</button>
												</div>
											</label>
										) : isRelayEditMode ? (
											<p className="field-note">
												Relay trust material comes from the pairing offer and is
												not edited here.
											</p>
										) : (
											<>
												<label className="field">
													<span>Relay URL</span>
													<input
														value={hostForm.relayUrl}
														onChange={(event) =>
															setHostForm((state) => ({
																...state,
																relayUrl: event.target.value,
															}))
														}
													/>
												</label>
												<label className="field">
													<span>Host ID</span>
													<input
														value={hostForm.hostId}
														onChange={(event) =>
															setHostForm((state) => ({
																...state,
																hostId: event.target.value,
															}))
														}
													/>
												</label>
												<label className="field">
													<span>Relay PSK</span>
													<input
														value={hostForm.relayPskHex}
														onChange={(event) =>
															setHostForm((state) => ({
																...state,
																relayPskHex: event.target.value,
															}))
														}
													/>
												</label>
											</>
										)}
										{hostForm.kind === "relay" &&
										!isRelayImportMode &&
										!isRelayEditMode ? (
											<label className="field">
												<span>Server public key</span>
												<input
													value={hostForm.serverPublicHex}
													onChange={(event) =>
														setHostForm((state) => ({
															...state,
															serverPublicHex: event.target.value,
														}))
													}
												/>
											</label>
										) : null}
										<div className="action-row">
											{!isRelayImportMode ? (
												<button type="submit">Save host</button>
											) : null}
											<button
												type="button"
												onClick={() => {
													setDetailMode("view");
													if (selectedHost != null) {
														setHostForm(initialFormState(selectedHost));
													}
												}}
											>
												Cancel
											</button>
										</div>
									</form>
								)}
							</section>
						) : null}

						{ui.overlaySection === "settings" ? (
							<section className="detail-section">
								<h2>Settings</h2>
								<div className="detail-grid">
									<div>
										<span>Theme</span>
										<strong>Dark</strong>
									</div>
									<div>
										<span>Shell</span>
										<strong>default</strong>
									</div>
									<div>
										<span>Scrollback</span>
										<strong>4194304</strong>
									</div>
									<div>
										<span>Keyboard</span>
										<strong>virtual key bar on touch input</strong>
									</div>
								</div>
							</section>
						) : null}

						{ui.overlaySection === "diagnostics" ? (
							<section className="detail-section">
								<h2>Diagnostics</h2>
								<div className="detail-grid">
									<div>
										<span>Connection state</span>
										<strong>{workspace.connectionState}</strong>
									</div>
									<div>
										<span>Active host</span>
										<strong>{activeHost?.label ?? "none"}</strong>
									</div>
									<div>
										<span>Endpoint</span>
										<strong>
											{activeHost ? endpointLabel(activeHost) : "none"}
										</strong>
									</div>
									<div>
										<span>Last error</span>
										<strong>{workspace.lastError ?? "none"}</strong>
									</div>
									<div>
										<span>Client</span>
										<strong>{mobile ? "mobile" : clientKind}</strong>
									</div>
									<div>
										<span>Active terminal</span>
										<strong>{activeSession?.title ?? "none"}</strong>
									</div>
									<div>
										<span>Terminal count</span>
										<strong>{workspace.terminals.length}</strong>
									</div>
								</div>
								<div className="action-row">
									<button type="button" disabled>
										Copy diagnostics
									</button>
									<button
										type="button"
										onClick={() => {
											workspaceState.getState().clearError();
											setInlineError(null);
										}}
									>
										Clear errors
									</button>
								</div>
							</section>
						) : null}

						{ui.overlaySection === "about" ? (
							<section className="detail-section">
								<h2>About</h2>
								<div className="detail-grid">
									<div>
										<span>Version</span>
										<strong>0.1.0</strong>
									</div>
									<div>
										<span>Client</span>
										<strong>{mobile ? "mobile" : clientKind}</strong>
									</div>
									<div>
										<span>Protocol</span>
										<strong>v1</strong>
									</div>
								</div>
								<p>Self-hosted remote terminal client.</p>
							</section>
						) : null}
					</div>
				</aside>
			) : null}
		</Shell>
	);
}
