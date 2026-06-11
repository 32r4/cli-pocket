import { DismissibleLayer } from "@/shared/components/DismissibleLayer";

interface PairQrCodeModalProps {
	qrSvg: string | null;
	onClose: () => void;
}

export function PairQrCodeModal({ qrSvg, onClose }: PairQrCodeModalProps) {
	if (qrSvg == null) {
		return null;
	}

	const qrImageSrc = `data:image/svg+xml;utf8,${encodeURIComponent(qrSvg)}`;

	return (
		<div className="server-modal-backdrop">
			<DismissibleLayer
				className="qr-modal"
				aria-modal="true"
				aria-label="Pair QR code"
				focusKey={qrSvg}
				onDismiss={onClose}
			>
				<div className="qr-code-surface">
					<img src={qrImageSrc} alt="Pair QR code" />
				</div>
				<div className="action-row">
					<button type="button" onClick={onClose}>
						Close
					</button>
				</div>
			</DismissibleLayer>
		</div>
	);
}
