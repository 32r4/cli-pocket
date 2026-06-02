import type {
	ComponentPropsWithoutRef,
	FocusEvent,
	KeyboardEvent,
} from "react";
import { useLayoutEffect, useRef } from "react";

type DismissibleLayerProps = Omit<
	ComponentPropsWithoutRef<"div">,
	"onBlur" | "onKeyDown" | "role"
> & {
	onDismiss: () => void;
	focusKey?: string;
};

export function DismissibleLayer({
	onDismiss,
	focusKey,
	tabIndex = -1,
	...props
}: DismissibleLayerProps) {
	const layerRef = useRef<HTMLDivElement | null>(null);

	useLayoutEffect(() => {
		void focusKey;
		const layer = layerRef.current;
		if (layer == null) {
			return;
		}

		const focusTarget = layer.querySelector<HTMLElement>(
			'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
		);
		(focusTarget ?? layer).focus();
	}, [focusKey]);

	const handleBlur = (event: FocusEvent<HTMLDivElement>) => {
		const nextTarget = event.relatedTarget as Node | null;
		if (nextTarget != null && event.currentTarget.contains(nextTarget)) {
			return;
		}

		onDismiss();
	};

	const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
		if (event.key === "Escape") {
			event.preventDefault();
			event.stopPropagation();
			onDismiss();
		}
	};

	return (
		<div
			ref={layerRef}
			role="dialog"
			tabIndex={tabIndex}
			onBlur={handleBlur}
			onKeyDown={handleKeyDown}
			{...props}
		/>
	);
}
