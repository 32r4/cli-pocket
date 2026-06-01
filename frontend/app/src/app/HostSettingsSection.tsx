import { Copy, Moon, RefreshCw, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { Group, Input, NumberField } from "react-aria-components";
import type { ThemeName } from "@/state/ui/uiState";

interface HostSettingsSectionProps {
	hostAvailable: boolean;
	scrollbackBytes: number;
	onScrollbackBytesChange: (scrollbackBytes: number) => void;
	theme: ThemeName;
	onCopyPairUrl: () => void;
	onRestartLocalDaemon: () => void;
	onThemeChange: (theme: ThemeName) => void;
}

function SettingsRow({
	label,
	children,
}: {
	label: string;
	children: ReactNode;
}) {
	return (
		<div className="settings-row">
			<span className="settings-row__label">{label}</span>
			<div className="settings-row__value">{children}</div>
		</div>
	);
}

const MIB = 1024 * 1024;
const MIN_SCROLLBACK_MIB = 1;
const MAX_SCROLLBACK_MIB = 64;

export function HostSettingsSection({
	hostAvailable,
	scrollbackBytes,
	onScrollbackBytesChange,
	theme,
	onCopyPairUrl,
	onRestartLocalDaemon,
	onThemeChange,
}: HostSettingsSectionProps) {
	const isDarkTheme = theme === "dark";

	return (
		<section className="detail-section">
			<div className="settings-stack">
				<SettingsRow label="Appearance">
					<div className="appearance-toggle">
						<button
							type="button"
							className="icon-button appearance-toggle__button"
							aria-label="Light theme"
							aria-pressed={!isDarkTheme}
							data-active={!isDarkTheme}
							onClick={() => onThemeChange("light")}
						>
							<Sun aria-hidden="true" size={16} strokeWidth={1.75} />
						</button>
						<button
							type="button"
							className="icon-button appearance-toggle__button"
							aria-label="Dark theme"
							aria-pressed={isDarkTheme}
							data-active={isDarkTheme}
							onClick={() => onThemeChange("dark")}
						>
							<Moon aria-hidden="true" size={16} strokeWidth={1.75} />
						</button>
					</div>
				</SettingsRow>
				<SettingsRow label="Scrollback">
					<NumberField
						aria-label="Scrollback in MiB"
						value={Math.round(scrollbackBytes / MIB)}
						minValue={MIN_SCROLLBACK_MIB}
						maxValue={MAX_SCROLLBACK_MIB}
						step={1}
						commitBehavior="snap"
						formatOptions={{ maximumFractionDigits: 0, useGrouping: false }}
						isWheelDisabled
						onChange={(nextScrollbackMiB) => {
							onScrollbackBytesChange(nextScrollbackMiB * MIB);
						}}
					>
						<Group className="settings-row__scrollback-field">
							<Input
								className="settings-row__scrollback-input"
								inputMode="numeric"
							/>
							<span className="settings-row__scrollback-unit">MiB</span>
						</Group>
					</NumberField>
				</SettingsRow>
				{hostAvailable ? (
					<SettingsRow label="Pair URL">
						<button
							type="button"
							className="icon-button"
							aria-label="Copy pair URL"
							onClick={onCopyPairUrl}
						>
							<Copy aria-hidden="true" size={14} strokeWidth={1.75} />
						</button>
					</SettingsRow>
				) : null}
				{hostAvailable ? (
					<SettingsRow label="Restart daemon">
						<button
							type="button"
							className="icon-button"
							aria-label="Restart daemon"
							onClick={onRestartLocalDaemon}
						>
							<RefreshCw aria-hidden="true" size={14} strokeWidth={1.75} />
						</button>
					</SettingsRow>
				) : null}
			</div>
		</section>
	);
}
