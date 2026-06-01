import { Copy, Moon, RefreshCw, Sun } from "lucide-react";
import type { ReactNode } from "react";
import type { ThemeName } from "@/state/ui/uiState";

interface HostSettingsSectionProps {
	hostAvailable: boolean;
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

export function HostSettingsSection({
	hostAvailable,
	theme,
	onCopyPairUrl,
	onRestartLocalDaemon,
	onThemeChange,
}: HostSettingsSectionProps) {
	const isDarkTheme = theme === "dark";

	return (
		<section className="detail-section">
			<div className="settings-stack">
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
				<SettingsRow label="Appearance">
					<button
						type="button"
						className="appearance-toggle"
						aria-label={`Switch to ${isDarkTheme ? "light" : "dark"} theme`}
						aria-pressed={isDarkTheme}
						onClick={() => onThemeChange(isDarkTheme ? "light" : "dark")}
					>
						<Sun
							aria-hidden="true"
							className="appearance-toggle__icon"
							data-active={!isDarkTheme}
							size={16}
							strokeWidth={1.75}
						/>
						<Moon
							aria-hidden="true"
							className="appearance-toggle__icon"
							data-active={isDarkTheme}
							size={16}
							strokeWidth={1.75}
						/>
					</button>
				</SettingsRow>
				<SettingsRow label="Scrollback">
					<strong>4194304</strong>
				</SettingsRow>
			</div>
		</section>
	);
}
