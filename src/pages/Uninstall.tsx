import { useState, useMemo, useEffect } from "react";
import { Search, AppWindow, Folder, File } from "lucide-react";
import { useUninstallStore } from "../stores/uninstallStore";
import { useSettingsStore } from "../stores/settingsStore";
import { formatSize } from "../utils/format";
import { STALE_DAYS, daysSinceUsed, lastUsedLabel } from "../utils/relativeTime";
import { getAppIconByPath, revealInFinder, type AppInfo } from "../lib/tauri";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import walk1 from "../assets/cat-walking/cat_walking_01.png";
import walk2 from "../assets/cat-walking/cat_walking_02.png";
import walk3 from "../assets/cat-walking/cat_walking_03.png";
import walk4 from "../assets/cat-walking/cat_walking_04.png";
import walk5 from "../assets/cat-walking/cat_walking_05.png";
import "../styles/uninstall.css";

type SortMode = "size" | "name" | "unused";

const SORT_OPTIONS: { mode: SortMode; label: string }[] = [
  { mode: "size", label: "Size" },
  { mode: "name", label: "Name" },
  { mode: "unused", label: "Unused" },
];

const WALK_FRAMES = [walk1, walk2, walk3, walk4, walk5, walk4, walk3, walk2];

function displayPath(path: string): string {
  return path.replace(/^\/Users\/[^/]+(?=\/|$)/, "~");
}

function iconSrc(iconData: string): string {
  return iconData.startsWith("data:") ? iconData : `data:image/png;base64,${iconData}`;
}

function isStale(app: AppInfo): boolean {
  return app.last_used_secs != null && daysSinceUsed(app.last_used_secs) > STALE_DAYS;
}

function compareUnused(a: AppInfo, b: AppInfo): number {
  const au = a.last_used_secs;
  const bu = b.last_used_secs;
  if (au != null && bu != null && au !== bu) return au - bu;
  if (au == null && bu != null) return 1;
  if (au != null && bu == null) return -1;
  return b.size - a.size;
}

function appSubline(app: AppInfo): string {
  if (app.is_system) return "System app";
  if (app.last_used_secs != null) return lastUsedLabel(app.last_used_secs);
  if (app.version) return `Version ${app.version}`;
  return "";
}

function heroMeta(app: AppInfo): string {
  const parts: string[] = [];
  if (app.version) parts.push(`Version ${app.version}`);
  parts.push(
    app.last_used_secs != null
      ? lastUsedLabel(app.last_used_secs)
      : app.bundle_id || displayPath(app.path)
  );
  return parts.join(" · ");
}

function WalkingCat() {
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setFrame((f) => (f + 1) % WALK_FRAMES.length), 120);
    return () => clearInterval(t);
  }, []);
  return <img src={WALK_FRAMES[frame]} alt="" className="uninstall-done-cat" draggable={false} />;
}

function AppIcon({ src, name, size }: { src?: string; name: string; size: "row" | "hero" }) {
  if (src) {
    return (
      <img
        src={iconSrc(src)}
        alt={name}
        className={size === "row" ? "uninstall-row-icon" : "uninstall-hero-icon"}
        draggable={false}
      />
    );
  }
  return (
    <div className={size === "row" ? "uninstall-row-icon uninstall-icon-ph" : "uninstall-hero-icon uninstall-icon-ph"}>
      <AppWindow size={size === "row" ? 16 : 30} strokeWidth={1.4} />
    </div>
  );
}

interface RemovedInfo {
  name: string;
  path: string;
  includedAll: boolean;
}

function UninstallLayout() {
  const phase = useUninstallStore((s) => s.phase);
  const apps = useUninstallStore((s) => s.apps);
  const search = useUninstallStore((s) => s.search);
  const setSearch = useUninstallStore((s) => s.setSearch);
  const selectedApp = useUninstallStore((s) => s.selectedApp);
  const selectApp = useUninstallStore((s) => s.selectApp);
  const associatedFiles = useUninstallStore((s) => s.associatedFiles);
  const loadingFiles = useUninstallStore((s) => s.loadingFiles);
  const selectedFilePaths = useUninstallStore((s) => s.selectedFilePaths);
  const toggleFile = useUninstallStore((s) => s.toggleFile);
  const selectAllFiles = useUninstallStore((s) => s.selectAllFiles);
  const deselectAllFiles = useUninstallStore((s) => s.deselectAllFiles);
  const uninstall = useUninstallStore((s) => s.uninstall);
  const progress = useUninstallStore((s) => s.progress);
  const result = useUninstallStore((s) => s.result);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [sortMode, setSortMode] = useState<SortMode>("size");
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [showConfirm, setShowConfirm] = useState(false);
  const [removed, setRemoved] = useState<RemovedInfo | null>(null);

  const scanning = phase === "scanning";
  const removing = phase === "removing";
  const isDone = phase === "done";

  useEffect(() => {
    if (apps.length === 0) return;
    let cancelled = false;
    const iconMap: Record<string, string> = {};
    Promise.allSettled(
      apps.map(async (app) => {
        const icon = await getAppIconByPath(app.path);
        if (icon) iconMap[app.path] = icon;
      })
    ).then(() => {
      if (!cancelled) setIcons((prev) => ({ ...prev, ...iconMap }));
    });
    return () => { cancelled = true; };
  }, [apps]);

  const displayed = useMemo(() => {
    let list = apps;
    if (search) {
      const q = search.toLowerCase();
      list = list.filter((a) => a.name.toLowerCase().includes(q));
    }
    const compare =
      sortMode === "size"
        ? (a: AppInfo, b: AppInfo) => b.size - a.size
        : sortMode === "name"
          ? (a: AppInfo, b: AppInfo) => a.name.localeCompare(b.name)
          : compareUnused;
    return [...list].sort(compare);
  }, [apps, search, sortMode]);

  useEffect(() => {
    if (phase !== "list" || selectedApp) return;
    const first = displayed.find((a) => !a.is_system);
    if (first) selectApp(first);
  }, [phase, selectedApp, displayed, selectApp]);

  const associatedSize = useMemo(
    () => associatedFiles.reduce((sum, f) => sum + f.size, 0),
    [associatedFiles]
  );

  const selectedFilesSize = useMemo(
    () => associatedFiles.reduce((sum, f) => (selectedFilePaths.has(f.path) ? sum + f.size : sum), 0),
    [associatedFiles, selectedFilePaths]
  );

  const allFilesSelected =
    associatedFiles.length > 0 && selectedFilePaths.size === associatedFiles.length;

  const percent = progress && progress.items_total > 0
    ? Math.round((progress.items_done / progress.items_total) * 100)
    : 0;

  const startUninstall = () => {
    if (!selectedApp) return;
    setShowConfirm(false);
    setRemoved({
      name: selectedApp.name,
      path: selectedApp.path,
      includedAll: selectedFilePaths.size === associatedFiles.length,
    });
    uninstall(!useTrash);
  };

  const pickAnother = () => {
    useUninstallStore.setState({ phase: "list", result: null, progress: null });
    setRemoved(null);
  };

  const appGone = !!(result && removed && result.deleted_paths.includes(removed.path));
  const removedName = removed?.name ?? "App";

  return (
    <>
      <div className="uninstall-layout">
        <div className="uninstall-sidebar">
          <div className="uninstall-sidebar-controls">
            <div className="uninstall-search">
              <Search size={13} className="uninstall-search-icon" />
              <input
                type="text"
                className="uninstall-search-input"
                placeholder={scanning ? "Search apps" : `Search ${apps.length} apps`}
                value={search}
                disabled={scanning}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            <div className="uninstall-sort">
              {SORT_OPTIONS.map((opt) => (
                <button
                  key={opt.mode}
                  className={`uninstall-sort-opt${sortMode === opt.mode ? " on" : ""}`}
                  onClick={() => setSortMode(opt.mode)}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
          <div className="uninstall-list">
            {scanning ? (
              <div className="uninstall-list-note">
                <div className="spinner" />
                <span>Scanning installed apps…</span>
              </div>
            ) : apps.length === 0 ? (
              <div className="uninstall-list-note">
                <span>No installed applications found.</span>
              </div>
            ) : displayed.length === 0 ? (
              <div className="uninstall-list-note">
                <span>No apps match your search.</span>
              </div>
            ) : (
              displayed.map((app) => {
                const isActive = selectedApp?.path === app.path && !isDone;
                const isRemoving = removing && selectedApp?.path === app.path;
                const sub = appSubline(app);
                return (
                  <div
                    key={app.path}
                    className={`uninstall-row${isActive ? " active" : ""}${app.is_system ? " system" : ""}${isRemoving ? " removing" : ""}`}
                    onClick={() => {
                      if (app.is_system || removing) return;
                      if (isDone) pickAnother();
                      selectApp(app);
                    }}
                  >
                    <AppIcon src={icons[app.path]} name={app.name} size="row" />
                    <div className="uninstall-row-text">
                      <div className="uninstall-row-name" title={app.name}>{app.name}</div>
                      {sub && <div className="uninstall-row-sub">{sub}</div>}
                    </div>
                    <span className="uninstall-row-size">{formatSize(app.size)}</span>
                  </div>
                );
              })
            )}
          </div>
        </div>

        <div className="uninstall-detail">
          {!isDone && selectedApp && (
            <div key={selectedApp.path} className="uninstall-detail-body">
              <div className="uninstall-hero">
                <AppIcon src={icons[selectedApp.path]} name={selectedApp.name} size="hero" />
                <div className="uninstall-hero-text">
                  <div className="uninstall-hero-name">{selectedApp.name}</div>
                  <div className="uninstall-hero-meta">{heroMeta(selectedApp)}</div>
                </div>
                {(selectedApp.is_data_sensitive || isStale(selectedApp)) && (
                  <div className="uninstall-badges">
                    {isStale(selectedApp) && (
                      <span className="uninstall-badge">Unused for 90+ days</span>
                    )}
                    {selectedApp.is_data_sensitive && (
                      <span
                        className="uninstall-badge"
                        title="This app may store sensitive data (passwords, keys, VPN configs). Export your data before removing."
                      >
                        May hold sensitive data
                      </span>
                    )}
                  </div>
                )}
              </div>

              <div className="uninstall-stats">
                <div className="uninstall-stat">
                  <div className="uninstall-stat-v">{formatSize(selectedApp.size)}</div>
                  <div className="uninstall-stat-k">App bundle</div>
                </div>
                <div className="uninstall-stat">
                  <div className="uninstall-stat-v">{loadingFiles ? "…" : formatSize(associatedSize)}</div>
                  <div className="uninstall-stat-k">Associated files</div>
                </div>
                <div className="uninstall-stat">
                  <div className="uninstall-stat-v">{loadingFiles ? "…" : associatedFiles.length + 1}</div>
                  <div className="uninstall-stat-k">Locations found</div>
                </div>
              </div>

              <div className="uninstall-section">
                <span>What gets removed</span>
                {!loadingFiles && associatedFiles.length > 0 && !removing && (
                  <button
                    className="uninstall-section-toggle"
                    onClick={allFilesSelected ? deselectAllFiles : selectAllFiles}
                  >
                    {allFilesSelected ? "Deselect All" : "Select All"}
                  </button>
                )}
              </div>

              <div className="uninstall-files">
                <div className="uninstall-file bundle">
                  <span className="uninstall-file-check-spacer" />
                  <AppWindow size={14} className="uninstall-file-icon" />
                  <span className="uninstall-file-path" title={selectedApp.path}>
                    {displayPath(selectedApp.path)}
                  </span>
                  <span className="uninstall-file-tag">Bundle</span>
                  <span className="uninstall-file-size">{formatSize(selectedApp.size)}</span>
                </div>
                {loadingFiles ? (
                  <div className="uninstall-files-note">
                    <div className="spinner" />
                    <span>Searching for associated files…</span>
                  </div>
                ) : associatedFiles.length === 0 ? (
                  <div className="uninstall-files-note">
                    <span>No associated files found. Only the app bundle will be removed.</span>
                  </div>
                ) : (
                  associatedFiles.map((file) => {
                    const checked = selectedFilePaths.has(file.path);
                    return (
                      <label
                        key={file.path}
                        className={`uninstall-file${checked ? "" : " off"}`}
                        title={`${file.category} — ${file.path}`}
                      >
                        <input
                          type="checkbox"
                          className="checkbox"
                          checked={checked}
                          disabled={removing}
                          onChange={() => toggleFile(file.path)}
                        />
                        {file.is_dir ? (
                          <Folder size={14} className="uninstall-file-icon" />
                        ) : (
                          <File size={14} className="uninstall-file-icon" />
                        )}
                        <span className="uninstall-file-path">{displayPath(file.path)}</span>
                        <span className="uninstall-file-size">{formatSize(file.size)}</span>
                      </label>
                    );
                  })
                )}
              </div>

              <div className="uninstall-footer">
                <span className="uninstall-frees">
                  {removing
                    ? progress
                      ? `${formatSize(progress.bytes_freed)} reclaimed`
                      : "Starting…"
                    : `Frees ${formatSize(selectedApp.size + selectedFilesSize)}`}
                </span>
                <button
                  className="uninstall-remove"
                  disabled={removing || loadingFiles}
                  onClick={() => setShowConfirm(true)}
                >
                  {removing ? `Uninstalling… ${percent}%` : `Uninstall ${selectedApp.name}`}
                </button>
              </div>
            </div>
          )}

          {!isDone && !selectedApp && (
            <div className="uninstall-placeholder">
              {scanning ? (
                <div className="spinner" />
              ) : (
                <>
                  <AppWindow size={30} strokeWidth={1.4} />
                  <span>Select an app to see what gets removed</span>
                </>
              )}
            </div>
          )}

          {isDone && (
            <div className="uninstall-done">
              <WalkingCat />
              <div className="uninstall-done-title">
                {appGone ? `${removedName} is gone` : `${removedName} wasn't fully removed`}
              </div>
              <div className="uninstall-done-sub">
                {formatSize(result?.bytes_freed ?? 0)} reclaimed
                {appGone && removed?.includedAll ? ", leftovers included." : "."}
                {result && result.errors.length > 0 &&
                  ` ${result.errors.length} item${result.errors.length !== 1 ? "s" : ""} couldn't be removed.`}
              </div>
              {result && result.kept?.length > 0 && (
                <div className="uninstall-kept">
                  <div className="uninstall-kept-title">Kept on your Mac</div>
                  {result.kept.map((k) => (
                    <div key={k.path} className="uninstall-kept-row">
                      <div className="uninstall-kept-info">
                        <div className="uninstall-kept-label">
                          {k.label} <span className="uninstall-kept-size">{formatSize(k.size)}</span>
                        </div>
                        <div className="uninstall-kept-msg">{k.message}</div>
                      </div>
                      <button className="uninstall-kept-reveal" onClick={() => revealInFinder(k.path).catch(() => {})}>
                        Show
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <button className="uninstall-done-btn" onClick={pickAnother}>
                {appGone ? "Pick another app" : "Back to app"}
              </button>
            </div>
          )}
        </div>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={
          selectedApp
            ? `Remove ${selectedApp.name} and ${selectedFilePaths.size} associated file${selectedFilePaths.size !== 1 ? "s" : ""}?`
            : ""
        }
        onConfirm={startUninstall}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

export default function Uninstall() {
  const phase = useUninstallStore((s) => s.phase);
  const error = useUninstallStore((s) => s.error);
  const scanApps = useUninstallStore((s) => s.scanApps);

  useEffect(() => {
    if (phase === "idle" && !error) scanApps();
  }, [phase, error, scanApps]);

  return (
    <div className="uninstall-container">
      {error && <div className="uninstall-error">{error}</div>}
      <UninstallLayout />
    </div>
  );
}
