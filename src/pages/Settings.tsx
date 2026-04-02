import { useEffect, useState } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import {
  resetLifetimeStats,
  getStoragePath,
  getTotalBytesFreed,
  pickFolder,
} from "../lib/tauri";
import { formatSize } from "../utils/format";
import "../styles/settings.css";

const LARGE_FILE_OPTIONS = [50, 100, 250, 500, 1000];
const SCAN_DEPTH_OPTIONS = [4, 6, 8, 10, 12];

export default function Settings() {
  const settings = useSettingsStore((s) => s.settings);
  const loaded = useSettingsStore((s) => s.loaded);
  const load = useSettingsStore((s) => s.load);
  const setDryRun = useSettingsStore((s) => s.setDryRun);
  const setUseTrash = useSettingsStore((s) => s.setUseTrash);
  const setLargeFileThreshold = useSettingsStore((s) => s.setLargeFileThreshold);
  const setAnalyzeScanDepth = useSettingsStore((s) => s.setAnalyzeScanDepth);
  const addWhitelist = useSettingsStore((s) => s.addWhitelist);
  const removeWhitelist = useSettingsStore((s) => s.removeWhitelist);

  const [showInput, setShowInput] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [storagePath, setStoragePath] = useState("");
  const [totalFreed, setTotalFreed] = useState(0);
  const [statsReset, setStatsReset] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (!loaded) load();
    getStoragePath().then((v) => { if (!cancelled) setStoragePath(v); }).catch(() => {});
    getTotalBytesFreed().then((v) => { if (!cancelled) setTotalFreed(v); }).catch(() => {});
    return () => { cancelled = true; };
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

  const handleBrowse = async () => {
    const selected = await pickFolder();
    if (selected) {
      const trimmed = selected.replace(/\/$/, "");
      if (!settings.whitelist.includes(trimmed)) {
        await addWhitelist(trimmed);
      }
    }
  };

  const handleResetStats = async () => {
    await resetLifetimeStats();
    setTotalFreed(0);
    setStatsReset(true);
    setTimeout(() => setStatsReset(false), 2000);
  };

  const handleResetSettings = async () => {
    const { saveSettings } = await import("../lib/tauri");
    const defaults = {
      dry_run: false,
      whitelist: [],
      use_trash: false,
      large_file_threshold_mb: 100,
      analyze_scan_depth: 8,
    };
    await saveSettings(defaults);
    await load();
  };

  return (
    <div className="settings-container">
      <div className="settings-header">Settings</div>

      <div className="settings-scroll">
      {/* ── General ── */}
      <div className="settings-section">
        <div className="settings-section-label">General</div>
        <div className="settings-card">
          <label className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Move to Trash</div>
              <div className="settings-row-desc">Send files to Trash instead of permanent deletion</div>
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
              <div className="settings-row-desc">Preview deletions without removing files</div>
            </div>
            <input
              type="checkbox"
              className="settings-toggle"
              checked={settings.dry_run}
              onChange={(e) => setDryRun(e.target.checked)}
            />
          </label>
        </div>
      </div>

      {/* ── Scanning ── */}
      <div className="settings-section">
        <div className="settings-section-label">Scanning</div>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Large File Threshold</div>
              <div className="settings-row-desc">Minimum size shown in Analyze → Large Files</div>
            </div>
            <select
              className="settings-select"
              value={settings.large_file_threshold_mb}
              onChange={(e) => setLargeFileThreshold(Number(e.target.value))}
            >
              {LARGE_FILE_OPTIONS.map((mb) => (
                <option key={mb} value={mb}>
                  {mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`}
                </option>
              ))}
            </select>
          </div>
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Analyze Scan Depth</div>
              <div className="settings-row-desc">Folder levels to scan. Higher values find more but take longer</div>
            </div>
            <select
              className="settings-select"
              value={settings.analyze_scan_depth}
              onChange={(e) => setAnalyzeScanDepth(Number(e.target.value))}
            >
              {SCAN_DEPTH_OPTIONS.map((d) => (
                <option key={d} value={d}>
                  {d} levels
                </option>
              ))}
            </select>
          </div>
        </div>
      </div>

      {/* ── Ignore List ── */}
      <div className="settings-section">
        <div className="settings-section-label">Ignore List</div>
        <div className="settings-card">
          <div className="settings-ignore-desc">
            Paths added here will never be cleaned or deleted by any module
          </div>

          {settings.whitelist.length > 0 && (
            <div className="settings-ignore-list">
              {settings.whitelist.map((path) => (
                <div key={path} className="settings-ignore-item">
                  <svg className="settings-ignore-icon" width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M14.5 13.5h-13a1 1 0 01-1-1v-8a1 1 0 011-1h4l2 2h7a1 1 0 011 1v6a1 1 0 01-1 1z" />
                  </svg>
                  <span className="settings-ignore-path">{path}</span>
                  <button
                    className="settings-ignore-remove"
                    onClick={() => removeWhitelist(path)}
                    title="Remove"
                  >
                    &times;
                  </button>
                </div>
              ))}
            </div>
          )}

          {showInput ? (
            <div className="settings-ignore-input-row">
              <input
                type="text"
                className="settings-ignore-input"
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
              <button className="btn" onClick={handleAddPath}>
                Add
              </button>
            </div>
          ) : (
            <div className="settings-ignore-actions">
              <button className="settings-ignore-add" onClick={() => setShowInput(true)}>
                + Type Path
              </button>
              <button className="settings-ignore-add" onClick={handleBrowse}>
                + Browse
              </button>
            </div>
          )}
        </div>
      </div>

      {/* ── Data ── */}
      <div className="settings-section">
        <div className="settings-section-label">Data</div>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Lifetime Stats</div>
              <div className="settings-row-desc">
                Total space reclaimed: {totalFreed > 0 ? formatSize(totalFreed) : "—"}
              </div>
            </div>
            <button
              className="btn settings-btn-sm"
              onClick={handleResetStats}
              disabled={statsReset}
            >
              {statsReset ? "Reset ✓" : "Reset"}
            </button>
          </div>
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Storage Location</div>
              <div className="settings-row-desc settings-mono">{storagePath || "—"}</div>
            </div>
          </div>
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Reset All Settings</div>
              <div className="settings-row-desc">Restore all settings to their defaults</div>
            </div>
            <button
              className="btn settings-btn-sm settings-btn-danger"
              onClick={handleResetSettings}
            >
              Reset
            </button>
          </div>
        </div>
      </div>

      {/* ── About ── */}
      <div className="settings-section">
        <div className="settings-section-label">About</div>
        <div className="settings-card settings-about">
          <div className="settings-about-name">Kyra</div>
          <div className="settings-about-version">v0.1.0</div>
          <div className="settings-about-desc">macOS Cleaner & Optimizer</div>
          <div className="settings-about-links">
            <a href="https://github.com/0xjba/Kyra" target="_blank" rel="noopener noreferrer">GitHub</a>
            <span className="settings-about-sep">·</span>
            <a href="https://github.com/0xjba/Kyra/blob/main/CHANGELOG.md" target="_blank" rel="noopener noreferrer">Changelog</a>
            <span className="settings-about-sep">·</span>
            <a href="https://github.com/0xjba/Kyra/blob/main/LICENSE" target="_blank" rel="noopener noreferrer">License</a>
          </div>
        </div>
      </div>
      </div>
    </div>
  );
}
