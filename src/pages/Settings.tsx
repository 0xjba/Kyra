import { useEffect } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import "../styles/settings.css";

export default function Settings() {
  const settings = useSettingsStore((s) => s.settings);
  const loaded = useSettingsStore((s) => s.loaded);
  const load = useSettingsStore((s) => s.load);
  const setDryRun = useSettingsStore((s) => s.setDryRun);

  useEffect(() => {
    if (!loaded) load();
  }, [loaded, load]);

  if (!loaded) return null;

  return (
    <div className="settings-container">
      <div className="settings-section">
        <div className="settings-section-label">Safety</div>

        <label className="settings-row">
          <div className="settings-row-info">
            <div className="settings-row-name">Dry Run Mode</div>
            <div className="settings-row-desc">
              Preview what would be deleted without actually removing files.
              Applies to Clean, Purge, Uninstall, and Installers.
            </div>
          </div>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={settings.dry_run}
            onChange={(e) => setDryRun(e.target.checked)}
          />
        </label>
      </div>

      <div className="settings-section">
        <div className="settings-section-label">About</div>

        <div className="settings-about">
          <div className="settings-about-name">Kyra</div>
          <div className="settings-about-version">v0.1.0</div>
          <div className="settings-about-desc">macOS Cleaner & Optimizer</div>
        </div>
      </div>
    </div>
  );
}
