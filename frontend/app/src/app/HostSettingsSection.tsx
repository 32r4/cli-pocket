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
					<strong>4194304</strong>
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
