interface ServerOptionButtonsProps {
	onOpenDirect: () => void;
	onOpenPairing: () => void;
}

export function ServerOptionButtons({
	onOpenDirect,
	onOpenPairing,
}: ServerOptionButtonsProps) {
	return (
		<>
			<button
				type="button"
				className="server-option-buttons__button"
				onClick={onOpenDirect}
			>
				Direct connection
			</button>
			<button
				type="button"
				className="server-option-buttons__button"
				onClick={onOpenPairing}
			>
				Pairing link
			</button>
			<button type="button" className="server-option-buttons__button" disabled>
				QR code
			</button>
		</>
	);
}
