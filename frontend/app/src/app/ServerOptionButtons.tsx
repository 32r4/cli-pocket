interface ServerOptionButtonsProps {
	onOpenDirect: () => void;
	onOpenPairing: () => void;
	onOpenQrScanner: () => void;
	showQrScanner: boolean;
}

export function ServerOptionButtons({
	onOpenDirect,
	onOpenPairing,
	onOpenQrScanner,
	showQrScanner,
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
			{showQrScanner ? (
				<button
					type="button"
					className="server-option-buttons__button"
					onClick={onOpenQrScanner}
				>
					QR code
				</button>
			) : null}
		</>
	);
}
