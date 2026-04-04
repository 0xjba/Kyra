import { useEffect, useState } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import {
  resetLifetimeStats,
  getStoragePath,
  getTotalBytesFreed,
  pickFolder,
} from "../lib/tauri";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { formatSize } from "../utils/format";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";
import logoSrc from "../assets/logo.png";
import "../styles/settings.css";

const LARGE_FILE_OPTIONS = [50, 100, 250, 500, 1000];
const SCAN_DEPTH_OPTIONS = [4, 6, 8, 10, 12];
const LOW_DISK_OPTIONS = [5, 10, 15, 20, 25];

export default function Settings() {
  const settings = useSettingsStore((s) => s.settings);
  const loaded = useSettingsStore((s) => s.loaded);
  const load = useSettingsStore((s) => s.load);
  const setDryRun = useSettingsStore((s) => s.setDryRun);
  const setUseTrash = useSettingsStore((s) => s.setUseTrash);
  const setLargeFileThreshold = useSettingsStore((s) => s.setLargeFileThreshold);
  const setAnalyzeScanDepth = useSettingsStore((s) => s.setAnalyzeScanDepth);
  const setLaunchAtLogin = useSettingsStore((s) => s.setLaunchAtLogin);
  const setCheckForUpdates = useSettingsStore((s) => s.setCheckForUpdates);
  const setNotificationsEnabled = useSettingsStore((s) => s.setNotificationsEnabled);
  const setLowDiskThreshold = useSettingsStore((s) => s.setLowDiskThreshold);
  const addWhitelist = useSettingsStore((s) => s.addWhitelist);
  const removeWhitelist = useSettingsStore((s) => s.removeWhitelist);

  const [showInput, setShowInput] = useState(false);
  const [newPath, setNewPath] = useState("");
  const [storagePath, setStoragePath] = useState("");
  const [totalFreed, setTotalFreed] = useState(0);
  const [statsReset, setStatsReset] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<"idle" | "checking" | "available" | "downloading" | "up-to-date">("idle");
  const [autoStartSynced, setAutoStartSynced] = useState(false);
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    let cancelled = false;
    if (!loaded) load();
    getStoragePath().then((v) => { if (!cancelled) setStoragePath(v); }).catch(() => {});
    getTotalBytesFreed().then((v) => { if (!cancelled) setTotalFreed(v); }).catch(() => {});
    getVersion().then((v) => { if (!cancelled) setAppVersion(v); }).catch(() => {});
    return () => { cancelled = true; };
  }, [loaded, load]);

  // Sync autostart toggle with actual OS state on mount
  useEffect(() => {
    if (loaded && !autoStartSynced) {
      isEnabled().then((enabled) => {
        if (enabled !== settings.launch_at_login) {
          setLaunchAtLogin(enabled);
        }
        setAutoStartSynced(true);
      }).catch(() => setAutoStartSynced(true));
    }
  }, [loaded, autoStartSynced, settings.launch_at_login, setLaunchAtLogin]);

  if (!loaded) return null;

  const handleAutoStartToggle = async (enabled: boolean) => {
    try {
      if (enabled) {
        await enable();
      } else {
        await disable();
      }
      await setLaunchAtLogin(enabled);
    } catch {
      // Revert on failure
    }
  };

  const handleCheckForUpdate = async () => {
    setUpdateStatus("checking");
    try {
      const update = await check();
      if (update) {
        setUpdateStatus("available");
      } else {
        setUpdateStatus("up-to-date");
        setTimeout(() => setUpdateStatus("idle"), 3000);
      }
    } catch {
      setUpdateStatus("idle");
    }
  };

  const handleDownloadUpdate = async () => {
    setUpdateStatus("downloading");
    try {
      const update = await check();
      if (update) {
        await update.downloadAndInstall();
        await relaunch();
      }
    } catch {
      setUpdateStatus("idle");
    }
  };

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
      launch_at_login: false,
      check_for_updates: true,
      notifications_enabled: true,
      low_disk_threshold_gb: 10,
      onboarding_completed: false,
    };
    // Also disable autostart if it was enabled
    try { await disable(); } catch {}
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
              <div className="settings-row-name">Launch at Login</div>
              <div className="settings-row-desc">Start Kyra automatically when you log in</div>
            </div>
            <input
              type="checkbox"
              className="settings-toggle"
              checked={settings.launch_at_login}
              onChange={(e) => handleAutoStartToggle(e.target.checked)}
            />
          </label>
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

      {/* ── Notifications & Updates ── */}
      <div className="settings-section">
        <div className="settings-section-label">Notifications & Updates</div>
        <div className="settings-card">
          <label className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Notifications</div>
              <div className="settings-row-desc">Enable system notifications for alerts</div>
            </div>
            <input
              type="checkbox"
              className="settings-toggle"
              checked={settings.notifications_enabled}
              onChange={(e) => setNotificationsEnabled(e.target.checked)}
            />
          </label>
          <label className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Check for Updates</div>
              <div className="settings-row-desc">Automatically check for updates on launch</div>
            </div>
            <input
              type="checkbox"
              className="settings-toggle"
              checked={settings.check_for_updates}
              onChange={(e) => setCheckForUpdates(e.target.checked)}
            />
          </label>
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Low Disk Space Alert</div>
              <div className="settings-row-desc">Warn when free space drops below threshold</div>
            </div>
            <select
              className="settings-select"
              value={settings.low_disk_threshold_gb}
              onChange={(e) => setLowDiskThreshold(Number(e.target.value))}
            >
              {LOW_DISK_OPTIONS.map((gb) => (
                <option key={gb} value={gb}>
                  {gb} GB
                </option>
              ))}
            </select>
          </div>
          <div className="settings-row">
            <div className="settings-row-info">
              <div className="settings-row-name">Software Update</div>
              <div className="settings-row-desc">
                {updateStatus === "checking" ? "Checking..." :
                 updateStatus === "available" ? "Update available" :
                 updateStatus === "downloading" ? "Downloading..." :
                 updateStatus === "up-to-date" ? "You're up to date" :
                 "Check for the latest version"}
              </div>
            </div>
            {updateStatus === "available" ? (
              <button className="btn settings-btn-sm" onClick={handleDownloadUpdate}>
                Update
              </button>
            ) : (
              <button
                className="btn settings-btn-sm"
                onClick={handleCheckForUpdate}
                disabled={updateStatus === "checking" || updateStatus === "downloading"}
              >
                {updateStatus === "checking" ? "Checking" :
                 updateStatus === "downloading" ? "Installing" :
                 updateStatus === "up-to-date" ? "Up to date" :
                 "Check Now"}
              </button>
            )}
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
        <div className="settings-about">
          <img src={logoSrc} alt="Kyra" className="settings-about-logo" />
          <div className="settings-about-name">Kyra <span className="settings-about-version">{appVersion ? `v${appVersion}` : ""}</span></div>
          <div className="settings-about-desc">NINE LIVES FOR YOUR STORAGE</div>
          <div className="settings-about-links">
            <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://github.com/0xjba/Kyra").catch(console.error); }}>GitHub</a>
            <span className="settings-about-sep">·</span>
            <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://github.com/0xjba/Kyra/blob/main/CHANGELOG.md").catch(console.error); }}>Changelog</a>
            <span className="settings-about-sep">·</span>
            <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://github.com/0xjba/Kyra/blob/main/LICENSE").catch(console.error); }}>License</a>
          </div>
          <div className="settings-about-dev">Developed by Jobin Ayathil</div>
        </div>
      </div>
      </div>
    </div>
  );
}
