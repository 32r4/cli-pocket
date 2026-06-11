import { useEffect, useRef, useState } from "react";

type DetectedBarcode = {
	rawValue: string;
};

type BarcodeDetectorConstructor = new (options: {
	formats: string[];
}) => {
	detect(source: CanvasImageSource): Promise<DetectedBarcode[]>;
};

interface PairingQrScannerProps {
	active: boolean;
	onDetected: (value: string) => void;
	onError: (message: string) => void;
}

function barcodeDetectorConstructor() {
	if (typeof window === "undefined" || !("BarcodeDetector" in window)) {
		return null;
	}

	return (window as Window & { BarcodeDetector: BarcodeDetectorConstructor })
		.BarcodeDetector;
}

export function PairingQrScanner({
	active,
	onDetected,
	onError,
}: PairingQrScannerProps) {
	const videoRef = useRef<HTMLVideoElement | null>(null);
	const [stream, setStream] = useState<MediaStream | null>(null);

	useEffect(() => {
		if (!active) {
			return;
		}

		const BarcodeDetector = barcodeDetectorConstructor();
		if (BarcodeDetector == null) {
			onError("QR scanning is unavailable on this WebView");
			return;
		}
		if (navigator.mediaDevices?.getUserMedia == null) {
			onError("camera unavailable");
			return;
		}

		let cancelled = false;
		let animationFrameId = 0;
		const detector = new BarcodeDetector({ formats: ["qr_code"] });

		const stopStream = (activeStream: MediaStream) => {
			for (const track of activeStream.getTracks()) {
				track.stop();
			}
		};

		void navigator.mediaDevices
			.getUserMedia({
				audio: false,
				video: { facingMode: { ideal: "environment" } },
			})
			.then((activeStream) => {
				if (cancelled) {
					stopStream(activeStream);
					return;
				}

				setStream(activeStream);
				const video = videoRef.current;
				if (video == null) {
					stopStream(activeStream);
					return;
				}

				video.srcObject = activeStream;
				const scan = async () => {
					if (cancelled) {
						return;
					}

					try {
						if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
							const results = await detector.detect(video);
							const rawValue = results[0]?.rawValue;
							if (rawValue != null && rawValue.trim().length > 0) {
								onDetected(rawValue);
								return;
							}
						}
					} catch (error: unknown) {
						onError(
							error instanceof Error ? error.message : "failed to scan QR code",
						);
						return;
					}

					animationFrameId = window.requestAnimationFrame(() => {
						void scan();
					});
				};

				void video.play().then(() => {
					void scan();
				});
			})
			.catch((error: unknown) => {
				onError(error instanceof Error ? error.message : "camera unavailable");
			});

		return () => {
			cancelled = true;
			if (animationFrameId !== 0) {
				window.cancelAnimationFrame(animationFrameId);
			}
			setStream((activeStream) => {
				if (activeStream != null) {
					stopStream(activeStream);
				}
				return null;
			});
		};
	}, [active, onDetected, onError]);

	return (
		<div className="qr-scanner">
			<video
				ref={videoRef}
				className="qr-scanner__video"
				muted
				playsInline
				data-active={stream != null}
			/>
			<div className="qr-scanner__frame" aria-hidden="true" />
		</div>
	);
}
