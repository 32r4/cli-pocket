import { PairingQrScanner } from "@/features/pairing/PairingQrScanner";
import { DismissibleLayer } from "@/shared/components/DismissibleLayer";
import type { DaemonRecord } from "@/state/daemon-registry/types";
import { ServerOptionButtons } from "./ServerOptionButtons";

export type ServerModalMode =
	| "closed"
	| "chooser"
	| "direct"
	| "pairing"
	| "qr";

export interface ServerFormState {
	kind: "direct" | "relay";
	directHost: string;
	directPort: string;
	relayUrl: string;
	serverId: string;
	relayPskHex: string;
	serverPublicHex: string;
}

export function initialFormState(): ServerFormState {
	return {
		kind: "direct",
		directHost: "127.0.0.1",
		directPort: "7842",
		relayUrl: "wss://relay.example/ws/client?server=",
		serverId: "",
		relayPskHex: "",
		serverPublicHex: "",
	};
}

export function makeServerRecord(form: ServerFormState): DaemonRecord {
	if (form.kind === "direct") {
		const id = crypto.randomUUID();
		const host = form.directHost.trim() || "127.0.0.1";
		const port = form.directPort.trim() || "7842";
		return {
			id,
			label: id,
			kind: "direct",
			endpointUrl: `ws://${host}:${port}/session`,
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
	showQrScanner: boolean;
	onClose: () => void;
	onOpenDirect: () => void;
	onOpenPairing: () => void;
	onOpenQrScanner: () => void;
	onSaveServer: () => void;
	onPairingUrlChange: (value: string) => void;
	onImportPairingLink: () => Promise<void>;
	onImportPairingLinkValue: (value: string) => Promise<void>;
	onPairingQrScannerError: (message: string) => void;
	onServerFormChange: (
		updater: (state: ServerFormState) => ServerFormState,
	) => void;
}

export function ServerModal({
	mode,
	serverForm,
	pairingUrl,
	showQrScanner,
	onClose,
	onOpenDirect,
	onOpenPairing,
	onOpenQrScanner,
	onSaveServer,
	onPairingUrlChange,
	onImportPairingLink,
	onImportPairingLinkValue,
	onPairingQrScannerError,
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
							onOpenQrScanner={onOpenQrScanner}
							showQrScanner={showQrScanner}
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
						<div className="direct-endpoint-fields">
							<label className="field">
								<span>Address</span>
								<input
									value={serverForm.directHost}
									autoComplete="off"
									onChange={(event) =>
										onServerFormChange((state) => ({
											...state,
											kind: "direct",
											directHost: event.target.value,
										}))
									}
								/>
							</label>
							<label className="field">
								<span>Port</span>
								<input
									value={serverForm.directPort}
									inputMode="numeric"
									autoComplete="off"
									onChange={(event) =>
										onServerFormChange((state) => ({
											...state,
											kind: "direct",
											directPort: event.target.value,
										}))
									}
								/>
							</label>
						</div>
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

				{mode === "qr" ? (
					<div className="server-form">
						<PairingQrScanner
							active={mode === "qr"}
							onDetected={(value) => {
								void onImportPairingLinkValue(value);
							}}
							onError={onPairingQrScannerError}
						/>
						<div className="action-row">
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
