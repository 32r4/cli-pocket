import { DismissibleLayer } from "@/shared/components/DismissibleLayer";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { ServerOptionButtons } from "./ServerOptionButtons";

export type ServerModalMode = "closed" | "chooser" | "direct" | "pairing";

export interface ServerFormState {
	kind: "direct" | "relay";
	endpointUrl: string;
	relayUrl: string;
	serverId: string;
	relayPskHex: string;
	serverPublicHex: string;
}

export function initialFormState(): ServerFormState {
	return {
		kind: "direct",
		endpointUrl: "",
		relayUrl: "wss://relay.example/ws/client?server=",
		serverId: "",
		relayPskHex: "",
		serverPublicHex: "",
	};
}

export function makeServerRecord(form: ServerFormState): DaemonRecord {
	if (form.kind === "direct") {
		const id = crypto.randomUUID();
		return {
			id,
			label: id,
			kind: "direct",
			endpointUrl: form.endpointUrl.trim(),
			resumeTokenHex: null,
			lastConnectedAt: null,
		};
	}

	const serverId = form.serverId.trim() || crypto.randomUUID();
	const serverPublicHex = form.serverPublicHex.trim() || "00".repeat(32);
	return {
		id: serverId,
		label: serverId,
		kind: "relay",
		serverId,
		relayUrl: form.relayUrl.trim(),
		relayPskHex: form.relayPskHex.trim() || "00".repeat(32),
		serverPublicHex,
		resumeTokenHex: null,
		lastConnectedAt: null,
	};
}

interface ServerModalProps {
	mode: ServerModalMode;
	serverForm: ServerFormState;
	pairingUrl: string;
	onClose: () => void;
	onOpenDirect: () => void;
	onOpenPairing: () => void;
	onSaveServer: () => void;
	onPairingUrlChange: (value: string) => void;
	onImportPairingLink: () => Promise<void>;
	onServerFormChange: (
		updater: (state: ServerFormState) => ServerFormState,
	) => void;
}

export function ServerModal({
	mode,
	serverForm,
	pairingUrl,
	onClose,
	onOpenDirect,
	onOpenPairing,
	onSaveServer,
	onPairingUrlChange,
	onImportPairingLink,
	onServerFormChange,
}: ServerModalProps) {
	if (mode === "closed") {
		return null;
	}

	return (
		<div className="server-modal-backdrop">
			<DismissibleLayer
				className="server-modal"
				aria-modal="true"
				aria-label="Server modal"
				focusKey={mode}
				onDismiss={onClose}
			>
				{mode === "chooser" ? (
					<div className="server-option-buttons">
						<ServerOptionButtons
							onOpenDirect={onOpenDirect}
							onOpenPairing={onOpenPairing}
						/>
					</div>
				) : null}

				{mode === "direct" ? (
					<form
						className="server-form"
						onSubmit={(event) => {
							event.preventDefault();
							onSaveServer();
						}}
					>
						<label className="field">
							<span>Endpoint URL</span>
							<input
								value={serverForm.endpointUrl}
								onChange={(event) =>
									onServerFormChange((state) => ({
										...state,
										kind: "direct",
										endpointUrl: event.target.value,
									}))
								}
							/>
						</label>
						<div className="action-row">
							<button type="submit">Save server</button>
							<button type="button" onClick={onClose}>
								Cancel
							</button>
						</div>
					</form>
				) : null}

				{mode === "pairing" ? (
					<div className="server-form">
						<label className="field">
							<span>Pairing link</span>
							<input
								value={pairingUrl}
								onChange={(event) => onPairingUrlChange(event.target.value)}
								placeholder="https://cli-pocket...#pair=..."
							/>
						</label>
						<div className="action-row">
							<button
								type="button"
								onClick={() => {
									void onImportPairingLink();
								}}
							>
								Import
							</button>
							<button type="button" onClick={onClose}>
								Cancel
							</button>
						</div>
					</div>
				) : null}
			</DismissibleLayer>
		</div>
	);
}
