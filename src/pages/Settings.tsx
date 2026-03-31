import { useEffect, useState } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import "../styles/settings.css";

export default function Settings() {
  const settings = useSettingsStore((s) => s.settings);
  const loaded = useSettingsStore((s) => s.loaded);
  const load = useSettingsStore((s) => s.load);
  const setDryRun = useSettingsStore((s) => s.setDryRun);
  const setUseTrash = useSettingsStore((s) => s.setUseTrash);
  const addWhitelist = useSettingsStore((s) => s.addWhitelist);
  const removeWhitelist = useSettingsStore((s) => s.removeWhitelist);

  const [showInput, setShowInput] = useState(false);
  const [newPath, setNewPath] = useState("");

  useEffect(() => {
    if (!loaded) load();
  }, [loaded, load]);

  if (!loaded) return null;

  const handleAddPath = async () => {
    const trimmed = newPath.trim();
    if (trimmed && !settings.whitelist.includes(trimmed)) {
      await addWhitelist(trimmed);
    }
    setNewPath("");
    setShowInput(false);
  };

  return (
    <div className="settings-container">
      <div className="settings-section">
        <div className="settings-section-label">Safety</div>

        <label className="settings-row">
          <div className="settings-row-info">
            <div className="settings-row-name">Move to Trash</div>
            <div className="settings-row-desc">
              Send deleted files to Trash instead of removing them permanently.
              Space is only freed after you empty Trash.
            </div>
          </div>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={settings.use_trash}
            onChange={(e) => setUseTrash(e.target.checked)}
          />
        </label>

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
        <div className="settings-section-label">Whitelist</div>

        <div className="settings-whitelist-card">
          <div className="settings-whitelist-desc">
            Paths added here will never be cleaned or deleted by any module.
          </div>

          {settings.whitelist.length > 0 && (
            <div className="settings-whitelist-list">
              {settings.whitelist.map((path) => (
                <div key={path} className="settings-whitelist-item">
                  <span className="settings-whitelist-path">{path}</span>
                  <button
                    className="settings-whitelist-remove"
                    onClick={() => removeWhitelist(path)}
                    title="Remove from whitelist"
                  >
                    &times;
                  </button>
                </div>
              ))}
            </div>
          )}

          {showInput ? (
            <div className="settings-whitelist-input-row">
              <input
                type="text"
                className="settings-whitelist-input"
                placeholder="/path/to/protect"
                value={newPath}
                onChange={(e) => setNewPath(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleAddPath();
                  if (e.key === "Escape") {
                    setShowInput(false);
                    setNewPath("");
                  }
                }}
                autoFocus
              />
              <button
                className="btn"
                onClick={handleAddPath}
              >
                Add
              </button>
            </div>
          ) : (
            <button
              className="settings-whitelist-add-trigger"
              onClick={() => setShowInput(true)}
            >
              + Add Path
            </button>
          )}
        </div>
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
