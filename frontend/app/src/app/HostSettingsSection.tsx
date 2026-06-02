import { Check, Copy, Moon, RefreshCw, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { Group, Input, NumberField } from "react-aria-components";
import type { ThemeName } from "@/state/ui/uiState";

interface HostSettingsSectionProps {
	scrollbackBytes: number | null;
	onScrollbackBytesChange: (scrollbackBytes: number) => void;
	theme: ThemeName;
	onCopyPairUrl: () => void;
	isPairUrlCopied: boolean;
	showPairControls: boolean;
	onRestartLocalDaemon: () => void;
	onThemeChange: (theme: ThemeName) => void;
}

function DetailRow({
	label,
	children,
}: {
	label: string;
	children: ReactNode;
}) {
	return (
		<div className="detail-row">
			<span className="detail-row__label">{label}</span>
			<div className="detail-row__value">{children}</div>
		</div>
	);
}

const MIB = 1024 * 1024;
const MIN_SCROLLBACK_MIB = 1;
const MAX_SCROLLBACK_MIB = 64;

export function HostSettingsSection({
	scrollbackBytes,
	onScrollbackBytesChange,
	theme,
	onCopyPairUrl,
	isPairUrlCopied,
	showPairControls,
	onRestartLocalDaemon,
	onThemeChange,
}: HostSettingsSectionProps) {
	const isDarkTheme = theme === "dark";

	return (
		<div className="detail-stack">
			<DetailRow label="Appearance">
				<div className="appearance-toggle">
					<button
						type="button"
						className="icon-button appearance-toggle__button"
						aria-label="Light theme"
						aria-pressed={!isDarkTheme}
						data-active={!isDarkTheme}
						onClick={() => onThemeChange("light")}
					>
						<Sun aria-hidden="true" size={14} strokeWidth={1.75} />
					</button>
					<button
						type="button"
						className="icon-button appearance-toggle__button"
						aria-label="Dark theme"
						aria-pressed={isDarkTheme}
						data-active={isDarkTheme}
						onClick={() => onThemeChange("dark")}
					>
						<Moon aria-hidden="true" size={14} strokeWidth={1.75} />
					</button>
				</div>
			</DetailRow>
			{scrollbackBytes != null ? (
				<DetailRow label="Scrollback">
					<NumberField
						aria-label="Scrollback in MiB"
						value={Math.round(scrollbackBytes / MIB)}
						minValue={MIN_SCROLLBACK_MIB}
						maxValue={MAX_SCROLLBACK_MIB}
						step={1}
						commitBehavior="snap"
						formatOptions={{
							maximumFractionDigits: 0,
							useGrouping: false,
						}}
						isWheelDisabled
						onChange={(nextScrollbackMiB) => {
							onScrollbackBytesChange(nextScrollbackMiB * MIB);
						}}
					>
						<Group className="detail-row__scrollback-field">
							<Input
								className="detail-row__scrollback-input"
								inputMode="numeric"
							/>
							<span className="detail-row__scrollback-unit">MiB</span>
						</Group>
					</NumberField>
				</DetailRow>
			) : null}
			{showPairControls ? (
				<DetailRow label="Pair URL">
					<button
						type="button"
						className="icon-button"
						aria-label={isPairUrlCopied ? "Pair URL copied" : "Copy pair URL"}
						data-copied={isPairUrlCopied}
						onClick={onCopyPairUrl}
					>
						{isPairUrlCopied ? (
							<Check aria-hidden="true" size={14} strokeWidth={1.75} />
						) : (
							<Copy aria-hidden="true" size={14} strokeWidth={1.75} />
						)}
					</button>
				</DetailRow>
			) : null}
			{showPairControls ? (
				<DetailRow label="Restart daemon">
					<button
						type="button"
						className="icon-button"
						aria-label="Restart daemon"
						onClick={onRestartLocalDaemon}
					>
						<RefreshCw aria-hidden="true" size={14} strokeWidth={1.75} />
					</button>
				</DetailRow>
			) : null}
		</div>
	);
}
