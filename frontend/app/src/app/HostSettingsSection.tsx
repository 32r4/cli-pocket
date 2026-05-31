import type { ThemeName } from "@/state/ui/uiState";

interface HostSettingsSectionProps {
	hostAvailable: boolean;
	localPairUrl: string | null;
	theme: ThemeName;
	onGenerateLocalPairUrl: () => void;
	onRestartLocalDaemon: () => void;
	onThemeChange: (theme: ThemeName) => void;
}

function themeLabel(theme: ThemeName) {
	return theme === "light" ? "Light" : "Dark";
}

export function HostSettingsSection({
	hostAvailable,
	localPairUrl,
	theme,
	onGenerateLocalPairUrl,
	onRestartLocalDaemon,
	onThemeChange,
}: HostSettingsSectionProps) {
	return (
		<section className="detail-section">
			<h2>Settings</h2>
			{hostAvailable ? (
				<div className="action-row">
					<button type="button" onClick={onGenerateLocalPairUrl}>
						Generate pair URL
					</button>
					<button type="button" onClick={onRestartLocalDaemon}>
						Restart daemon
					</button>
				</div>
			) : null}
			{hostAvailable && localPairUrl != null ? (
				<div className="detail-grid">
					<div>
						<span>Pair URL</span>
						<strong>{localPairUrl}</strong>
					</div>
				</div>
			) : null}
			<fieldset className="field-stack theme-fieldset">
				<legend className="sr-only">Theme preference</legend>
				<div className="action-row">
					<button
						type="button"
						data-active={theme === "dark"}
						className="theme-toggle"
						aria-label="Use dark theme"
						onClick={() => onThemeChange("dark")}
					>
						Dark
					</button>
					<button
						type="button"
						data-active={theme === "light"}
						className="theme-toggle"
						aria-label="Use light theme"
						onClick={() => onThemeChange("light")}
					>
						Light
					</button>
				</div>
			</fieldset>
			<div className="detail-grid">
				<div>
					<span>Theme</span>
					<strong>{themeLabel(theme)}</strong>
				</div>
				<div>
					<span>Shell</span>
					<strong>default</strong>
				</div>
				<div>
					<span>Scrollback</span>
					<strong>4194304</strong>
				</div>
				<div>
					<span>Keyboard</span>
					<strong>virtual key bar on touch input</strong>
				</div>
			</div>
		</section>
	);
}
