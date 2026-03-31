import { useState, useCallback, useMemo, useEffect } from "react";
import {
  Search,
  Info,
  AlertTriangle,
  Check,
  AppWindow,
} from "lucide-react";
import { useUninstallStore } from "../stores/uninstallStore";
import { useSettingsStore } from "../stores/settingsStore";
import { useNavigationStore } from "../stores/navigationStore";
import { formatSize } from "../utils/format";
import { getAppIconByPath } from "../lib/tauri";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import "../styles/uninstall.css";

/* ── Helpers ────────────────────────────────────────────── */



/* ── Sort modes ── */
type SortMode = "size" | "name";

/* ── Filter chips ── */
type FilterMode = "all" | "large" | "dev" | "creative" | "comms";

const FILTER_LABELS: Record<FilterMode, string> = {
  all: "All",
  large: "Large (>500 MB)",
  dev: "Dev Tools",
  creative: "Creative",
  comms: "Communication",
};

const DEV_KEYWORDS = [
  "xcode", "docker", "android studio", "visual studio", "vs code",
  "intellij", "webstorm", "phpstorm", "pycharm", "clion", "rider",
  "goland", "rubymine", "datagrip", "fleet", "sublime", "atom",
  "nova", "bbedit", "coteditor", "tower", "fork", "sourcetree",
  "github", "iterm", "terminal", "warp", "alacritty", "kitty",
  "hyper", "postman", "insomnia", "httpie", "tableplus", "sequel",
  "postico", "pgadmin", "mongodb", "redis", "homebrew", "cursor",
  "zed", "neovide", "fig", "dash", "paw", "proxyman", "charles",
  "wireshark", "transmit", "cyberduck", "filezilla", "unity",
  "unreal", "godot", "utm",
];

const CREATIVE_KEYWORDS = [
  "photoshop", "illustrator", "indesign", "lightroom", "premiere",
  "after effects", "adobe", "figma", "sketch", "affinity",
  "pixelmator", "procreate", "canva", "blender", "cinema 4d",
  "davinci", "final cut", "imovie", "logic pro", "garageband",
  "ableton", "fl studio", "audacity", "obs", "screenflow",
  "capcut", "clipchamp", "handbrake", "vlc", "iina", "compressor",
  "motion", "keynote", "pages", "acorn", "darkroom", "halide",
  "luminar", "capture one", "color", "preview",
];

const COMMS_KEYWORDS = [
  "slack", "discord", "teams", "zoom", "telegram", "whatsapp",
  "signal", "messenger", "skype", "webex", "google meet",
  "facetime", "messages", "mail", "outlook", "thunderbird",
  "spark", "airmail", "mimestream", "canary", "hey", "superhuman",
  "loom", "around", "gather", "linear", "notion", "basecamp",
  "clickup", "asana", "trello", "monday", "intercom", "crisp",
  "front", "missive", "beeper", "element", "revolt",
];

function matchesFilter(name: string, filter: FilterMode, size: number): boolean {
  if (filter === "all") return true;
  if (filter === "large") return size >= 500 * 1024 * 1024;
  const lower = name.toLowerCase();
  if (filter === "dev") return DEV_KEYWORDS.some((k) => lower.includes(k));
  if (filter === "creative") return CREATIVE_KEYWORDS.some((k) => lower.includes(k));
  if (filter === "comms") return COMMS_KEYWORDS.some((k) => lower.includes(k));
  return true;
}


/* ═══════════════════════════════════════════════════════════
   Scanning View
   ═══════════════════════════════════════════════════════════ */

function ScanningView() {
  return (
    <div className="uninstall-centered">
      <div className="uninstall-spinner" />
      <div className="uninstall-scanning-text">Scanning installed applications...</div>
    </div>
  );
}

/* ═══════════════════════════════════════════════════════════
   App Grid View (Launchpad-style)
   ═══════════════════════════════════════════════════════════ */

function AppGridView() {
  const apps = useUninstallStore((s) => s.apps);
  const search = useUninstallStore((s) => s.search);
  const setSearch = useUninstallStore((s) => s.setSearch);
  const selectApp = useUninstallStore((s) => s.selectApp);
  const bulkUninstall = useUninstallStore((s) => s.bulkUninstall);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [sortMode, setSortMode] = useState<SortMode>("size");
  const [filterMode, setFilterMode] = useState<FilterMode>("all");
  const [selectedApps, setSelectedApps] = useState<Set<string>>(new Set());
  const [showConfirm, setShowConfirm] = useState(false);
  const [icons, setIcons] = useState<Record<string, string>>({});

  // Fetch app icons on mount
  useEffect(() => {
    const iconMap: Record<string, string> = {};
    Promise.allSettled(
      apps.map(async (app) => {
        const icon = await getAppIconByPath(app.path);
        if (icon) iconMap[app.name] = icon;
      })
    ).then(() => {
      setIcons({ ...iconMap });
    });
  }, [apps]);

  // Filter counts for chips
  const filterCounts = useMemo(() => ({
    large: apps.filter((a) => a.size >= 500 * 1024 * 1024).length,
    dev: apps.filter((a) => DEV_KEYWORDS.some((k) => a.name.toLowerCase().includes(k))).length,
    creative: apps.filter((a) => CREATIVE_KEYWORDS.some((k) => a.name.toLowerCase().includes(k))).length,
    comms: apps.filter((a) => COMMS_KEYWORDS.some((k) => a.name.toLowerCase().includes(k))).length,
  }), [apps]);

  // Filtered + sorted apps
  const displayed = useMemo(() => {
    let list = apps;

    // Search filter
    if (search) {
      const q = search.toLowerCase();
      list = list.filter((a) => a.name.toLowerCase().includes(q));
    }

    // Category filter
    list = list.filter((a) => matchesFilter(a.name, filterMode, a.size));

    // Sort
    list = [...list].sort((a, b) => {
      if (sortMode === "size") return b.size - a.size;
      return a.name.localeCompare(b.name);
    });

    return list;
  }, [apps, search, filterMode, sortMode]);

  const totalSize = useMemo(
    () => apps.reduce((sum, a) => sum + a.size, 0),
    [apps]
  );

  // Selection helpers
  const toggleApp = useCallback((path: string) => {
    setSelectedApps((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const nonSystemDisplayed = useMemo(
    () => displayed.filter((a) => !a.is_system),
    [displayed]
  );

  const allSelected = nonSystemDisplayed.length > 0 &&
    nonSystemDisplayed.every((a) => selectedApps.has(a.path));

  const toggleSelectAll = useCallback(() => {
    if (allSelected) {
      setSelectedApps(new Set());
    } else {
      setSelectedApps(new Set(nonSystemDisplayed.map((a) => a.path)));
    }
  }, [allSelected, nonSystemDisplayed]);

  const selectedSize = useMemo(
    () => apps.filter((a) => selectedApps.has(a.path)).reduce((sum, a) => sum + a.size, 0),
    [apps, selectedApps]
  );

  // Handle uninstall click
  const handleUninstallClick = useCallback(() => {
    if (selectedApps.size === 0) return;
    if (selectedApps.size === 1) {
      // Single app — go to detail view for file review
      const appPath = Array.from(selectedApps)[0];
      const app = apps.find((a) => a.path === appPath);
      if (app) selectApp(app);
    } else {
      // Multiple apps — show confirmation dialog
      setShowConfirm(true);
    }
  }, [selectedApps, apps, selectApp]);

  // Bulk uninstall confirmed
  const handleConfirmBulk = useCallback(() => {
    setShowConfirm(false);
    const appsToRemove = apps.filter((a) => selectedApps.has(a.path));
    bulkUninstall(appsToRemove, !useTrash);
    setSelectedApps(new Set());
  }, [apps, selectedApps, bulkUninstall, useTrash]);

  // Empty state
  if (apps.length === 0) {
    return (
      <div className="uninstall-centered">
        <div className="uninstall-empty-icon">
          <Check size={26} strokeWidth={1.5} />
        </div>
        <div className="uninstall-idle-title">No applications found</div>
        <div className="uninstall-idle-desc">
          No installed applications were detected on this system.
        </div>
      </div>
    );
  }

  return (
    <>
      {/* Header */}
      <div className="uninstall-list-header">
        <div className="uninstall-list-summary">
          <span className="uninstall-list-title">Uninstall</span>
          <span className="uninstall-list-context">
            {apps.length} apps · {formatSize(totalSize)}
          </span>
        </div>
        <div className="uninstall-list-actions">
          <button
            className="btn"
            style={{ minWidth: 90 }}
            onClick={toggleSelectAll}
          >
            {allSelected ? "Deselect All" : "Select All"}
          </button>
        </div>
      </div>

      {/* Search + Sort Row */}
      <div className="uninstall-search-row">
        <div className="uninstall-search-box">
          <Search size={13} className="uninstall-search-icon" />
          <input
            type="text"
            className="uninstall-search-input"
            placeholder="Search apps..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className="uninstall-sort-controls">
          <span className="uninstall-sort-label">Sort</span>
          <button
            className={`uninstall-sort-btn ${sortMode === "size" ? "active" : ""}`}
            onClick={() => setSortMode("size")}
          >
            Size
          </button>
          <button
            className={`uninstall-sort-btn ${sortMode === "name" ? "active" : ""}`}
            onClick={() => setSortMode("name")}
          >
            Name
          </button>
        </div>
      </div>

      {/* Filter Chips */}
      <div className="uninstall-quick-filters">
        {(Object.keys(FILTER_LABELS) as FilterMode[]).map((f) => (
          <button
            key={f}
            className={`uninstall-filter-btn ${filterMode === f ? "active" : ""}`}
            onClick={() => setFilterMode(f)}
          >
            {FILTER_LABELS[f]}
            {f !== "all" && (filterCounts as any)[f] > 0 && (
              <span className="uninstall-filter-count">{(filterCounts as any)[f]}</span>
            )}
          </button>
        ))}
      </div>

      {/* App Grid */}
      <div className="uninstall-grid-scroll">
        {displayed.length === 0 ? (
          <div className="uninstall-centered" style={{ minHeight: 200 }}>
            <div className="uninstall-idle-desc">No apps match your search or filter.</div>
          </div>
        ) : (
          <div className="uninstall-grid">
            {displayed.map((app) => {
              const isSystem = app.is_system;
              const isSelected = selectedApps.has(app.path);
              const icon = icons[app.name];

              return (
                <div
                  key={app.path}
                  className={`uninstall-card${isSystem ? " uninstall-card-system" : ""}${isSelected ? " uninstall-card-selected" : ""}`}
                >
                  {/* Checkbox — top left */}
                  {!isSystem && (
                    <input
                      type="checkbox"
                      className="checkbox uninstall-card-checkbox"
                      checked={isSelected}
                      onChange={() => toggleApp(app.path)}
                    />
                  )}

                  {/* Info button — top right */}
                  {!isSystem && (
                    <button
                      className="uninstall-card-info-btn"
                      onClick={(e) => {
                        e.stopPropagation();
                        selectApp(app);
                      }}
                      title="View details"
                    >
                      <Info size={13} strokeWidth={1.8} />
                    </button>
                  )}

                  {/* App icon */}
                  <div
                    className="uninstall-card-icon"
                    onClick={() => !isSystem && toggleApp(app.path)}
                  >
                    {icon ? (
                      <img
                        src={icon.startsWith("data:") ? icon : `data:image/png;base64,${icon}`}
                        alt={app.name}
                        className="uninstall-card-icon-img"
                        draggable={false}
                      />
                    ) : (
                      <div className="uninstall-card-icon-placeholder">
                        <AppWindow size={22} strokeWidth={1.3} />
                      </div>
                    )}
                  </div>

                  {/* App name */}
                  <div
                    className="uninstall-card-name"
                    title={app.name}
                    onClick={() => !isSystem && toggleApp(app.path)}
                  >
                    {app.name}
                  </div>

                  {/* Size */}
                  <div className="uninstall-card-size">{formatSize(app.size)}</div>

                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="uninstall-footer">
        <span className="uninstall-footer-info">
          {selectedApps.size} of {apps.filter((a) => !a.is_system).length} apps selected
        </span>
        <button
          className="btn btn-primary"
          style={{ minWidth: 120 }}
          disabled={selectedApps.size === 0}
          onClick={handleUninstallClick}
        >
          Uninstall{selectedApps.size > 0 ? ` ${formatSize(selectedSize)}` : ""}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Uninstall ${selectedApps.size} apps and all associated files (${formatSize(selectedSize)})?`}
        onConfirm={handleConfirmBulk}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ── Category → short badge label mapping ── */
const CATEGORY_BADGE: Record<string, string> = {
  "App Data": "app-support",
  "Preferences": "pref",
  "Caches": "cache",
  "Containers": "container",
  "Group Containers": "group",
  "Logs": "log",
  "Saved State": "saved-state",
  "WebKit Data": "webkit",
  "HTTP Storage": "http",
  "Launch Agents": "launch-agent",
  "Launch Daemons": "daemon",
  "Cookies": "cookie",
  "Crash Reports": "crash",
  "Receipts": "receipt",
  "Plug-ins": "plugin",
  "Login Items": "login-item",
};

/** Extract the last path component (filename or folder name). */
function fileName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

/* ═══════════════════════════════════════════════════════════
   Detail View — Single app with associated files
   ═══════════════════════════════════════════════════════════ */

function DetailView() {
  const selectedApp = useUninstallStore((s) => s.selectedApp);
  const associatedFiles = useUninstallStore((s) => s.associatedFiles);
  const loadingFiles = useUninstallStore((s) => s.loadingFiles);
  const selectedFilePaths = useUninstallStore((s) => s.selectedFilePaths);
  const toggleFile = useUninstallStore((s) => s.toggleFile);
  const selectAllFiles = useUninstallStore((s) => s.selectAllFiles);
  const deselectAllFiles = useUninstallStore((s) => s.deselectAllFiles);
  const deselectApp = useUninstallStore((s) => s.deselectApp);
  const uninstall = useUninstallStore((s) => s.uninstall);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);
  const setBackOverride = useNavigationStore((s) => s.setBackOverride);
  const [showConfirm, setShowConfirm] = useState(false);
  const [icon, setIcon] = useState<string | null>(null);

  // Wire titlebar back button to go back to grid
  useEffect(() => {
    setBackOverride(deselectApp);
    return () => setBackOverride(null);
  }, [deselectApp, setBackOverride]);

  // Fetch icon
  useEffect(() => {
    if (!selectedApp) return;
    getAppIconByPath(selectedApp.path).then((ic) => {
      if (ic) setIcon(ic);
    });
    return () => setIcon(null);
  }, [selectedApp]);

  if (!selectedApp) return null;

  const associatedSize = associatedFiles.reduce((sum, f) => sum + f.size, 0);

  // Unique location count (distinct category directories)
  const locationCount = new Set(associatedFiles.map((f) => f.category)).size;

  // All items = app bundle + associated files
  const allItems = useMemo(() => {
    const bundle = {
      path: selectedApp.path,
      category: "Bundle",
      size: selectedApp.size,
      is_dir: true,
      isBundleEntry: true,
    };
    return [bundle, ...associatedFiles.map((f) => ({ ...f, isBundleEntry: false }))];
  }, [selectedApp, associatedFiles]);

  const totalItemCount = allItems.length;

  // Bundle is always "selected" — count = selected files + 1 for bundle
  const selectedCount = selectedFilePaths.size + 1;
  const allSelected =
    associatedFiles.length > 0 &&
    selectedFilePaths.size === associatedFiles.length;

  return (
    <>
      {/* ── Centered hero header ── */}
      <div className="uninstall-detail-hero">
        <div className="uninstall-detail-icon-wrap">
          {icon ? (
            <img
              src={icon.startsWith("data:") ? icon : `data:image/png;base64,${icon}`}
              alt={selectedApp.name}
              className="uninstall-detail-icon-img"
              draggable={false}
            />
          ) : (
            <div className="uninstall-detail-icon-placeholder">
              <AppWindow size={32} strokeWidth={1.3} />
            </div>
          )}
        </div>
        <div className="uninstall-detail-name">{selectedApp.name}</div>
        <div className="uninstall-detail-meta">
          {selectedApp.version && `v${selectedApp.version} · `}
          {selectedApp.path}
        </div>
      </div>

      {/* ── Stats row ── */}
      <div className="uninstall-detail-stats">
        <div className="uninstall-detail-stat">
          <span className="uninstall-detail-stat-value">{formatSize(selectedApp.size)}</span>
          <span className="uninstall-detail-stat-label">App bundle</span>
        </div>
        <div className="uninstall-detail-stat">
          <span className="uninstall-detail-stat-value">{formatSize(associatedSize)}</span>
          <span className="uninstall-detail-stat-label">Associated files</span>
        </div>
        <div className="uninstall-detail-stat">
          <span className="uninstall-detail-stat-value">{locationCount}</span>
          <span className="uninstall-detail-stat-label">Locations found</span>
        </div>
      </div>

      {/* Sensitive data warning */}
      {selectedApp.is_data_sensitive && (
        <div className="uninstall-sensitive-warning">
          <AlertTriangle size={13} />
          <span>This app may store sensitive data (passwords, keys, VPN configs). Export your data before removing.</span>
        </div>
      )}

      {/* ── Section heading ── */}
      <div className="uninstall-detail-section-header">
        <span className="uninstall-detail-section-title">What gets removed</span>
        {associatedFiles.length > 0 && (
          <button
            className="btn"
            style={{ minWidth: 90 }}
            onClick={allSelected ? deselectAllFiles : selectAllFiles}
          >
            {allSelected ? "Deselect All" : "Select All"}
          </button>
        )}
      </div>

      {/* ── File list ── */}
      <div className="uninstall-file-list">
        {loadingFiles ? (
          <div className="uninstall-centered" style={{ minHeight: 120 }}>
            <div className="uninstall-spinner" />
            <div className="uninstall-scanning-text">Searching for associated files...</div>
          </div>
        ) : (
          <>
            {/* App bundle row — always included */}
            <div className="uninstall-file-row uninstall-file-row-bundle">
              <span className="uninstall-file-name">
                {fileName(selectedApp.path)}
              </span>
              <span className="uninstall-file-badge">bundle</span>
              <span className="uninstall-file-size">
                {formatSize(selectedApp.size)}
              </span>
            </div>

            {/* Associated files — flat list with category badges */}
            {associatedFiles.length === 0 ? (
              <div className="uninstall-file-empty">
                No associated files found. Only the app bundle will be removed.
              </div>
            ) : (
              associatedFiles.map((file) => (
                <label key={file.path} className="uninstall-file-row">
                  <input
                    type="checkbox"
                    className="checkbox"
                    checked={selectedFilePaths.has(file.path)}
                    onChange={() => toggleFile(file.path)}
                  />
                  <span className="uninstall-file-name" title={file.path}>
                    {fileName(file.path)}
                  </span>
                  <span className="uninstall-file-badge">
                    {CATEGORY_BADGE[file.category] || file.category.toLowerCase()}
                  </span>
                  <span className="uninstall-file-size">
                    {formatSize(file.size)}
                  </span>
                </label>
              ))
            )}
          </>
        )}
      </div>

      {/* ── Footer ── */}
      <div className="uninstall-footer">
        <span className="uninstall-footer-info">
          {selectedCount} of {totalItemCount} items selected
        </span>
        <button
          className="btn btn-primary"
          style={{ minWidth: 120 }}
          onClick={() => setShowConfirm(true)}
        >
          Remove {selectedApp.name}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Remove ${selectedApp.name} and ${selectedFilePaths.size} associated file${selectedFilePaths.size !== 1 ? "s" : ""}?`}
        onConfirm={() => { setShowConfirm(false); uninstall(!useTrash); }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ═══════════════════════════════════════════════════════════
   Removing View — Ring progress + Done state
   ═══════════════════════════════════════════════════════════ */

function RemovingView() {
  const progress = useUninstallStore((s) => s.progress);
  const result = useUninstallStore((s) => s.result);
  const isDone = result !== null;

  const handleDone = useCallback(() => {
    useUninstallStore.setState({ phase: "list", result: null, progress: null });
  }, []);

  // Done state
  if (isDone && result) {
    return (
      <div className="uninstall-centered">
        <div className="uninstall-done-check">
          <Check size={20} strokeWidth={2.5} />
        </div>
        <div className="uninstall-summary-stat">
          {formatSize(result.bytes_freed)}
        </div>
        <div className="uninstall-summary-label">space reclaimed</div>
        <div className="uninstall-summary-detail">
          {result.items_removed} item{result.items_removed !== 1 ? "s" : ""} removed
        </div>
        {result.errors.length > 0 && (
          <div className="uninstall-done-errors">
            <span
              className="uninstall-error-chip"
              title={result.errors.map((e) => {
                const parts = e.split(": ");
                if (parts.length >= 2) {
                  const filePart = parts[0].split("/").pop() || parts[0];
                  return `${filePart}: ${parts.slice(1).join(": ")}`;
                }
                return e;
              }).join("\n")}
            >
              {result.errors.length} error{result.errors.length > 1 ? "s" : ""}
            </span>
          </div>
        )}
        <button className="btn" style={{ minWidth: 100, marginTop: 16 }} onClick={handleDone}>
          Done
        </button>
      </div>
    );
  }

  // Progress state — ring
  const percent = progress && progress.items_total > 0
    ? Math.round((progress.items_done / progress.items_total) * 100)
    : 0;

  const ringSize = 120;
  const strokeWidth = 6;
  const radius = (ringSize - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference - (percent / 100) * circumference;

  const currentLabel = progress?.current_item
    ? `Removing ${progress.current_item.split("/").filter(Boolean).pop() || progress.current_item}...`
    : "Starting...";

  return (
    <div className="uninstall-centered">
      {/* Progress ring */}
      <div className="uninstall-ring-wrap">
        <svg
          className="uninstall-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="uninstall-ring-glass" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.35)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.18)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.30)" />
            </linearGradient>
          </defs>
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none" stroke="rgba(255, 255, 255, 0.06)" strokeWidth={strokeWidth}
          />
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none"
            stroke="url(#uninstall-ring-glass)"
            strokeWidth={strokeWidth} strokeLinecap="round"
            strokeDasharray={circumference} strokeDashoffset={dashOffset}
            className="uninstall-ring-fill"
          />
        </svg>
        <span className="uninstall-ring-percent">{percent}%</span>
      </div>

      <div className="uninstall-ring-freed">
        {progress ? `${formatSize(progress.bytes_freed)} reclaimed` : "0 B reclaimed"}
      </div>

      <div className="uninstall-ring-current">{currentLabel}</div>
    </div>
  );
}

/* ═══════════════════════════════════════════════════════════
   Main Component
   ═══════════════════════════════════════════════════════════ */

export default function Uninstall() {
  const phase = useUninstallStore((s) => s.phase);
  const error = useUninstallStore((s) => s.error);
  const result = useUninstallStore((s) => s.result);
  const selectedApp = useUninstallStore((s) => s.selectedApp);
  const scanApps = useUninstallStore((s) => s.scanApps);

  useEffect(() => {
    if (phase === "idle") {
      scanApps();
    }
  }, [phase, scanApps]);

  // Show removing/done view only when actively removing
  const showRemoving = phase === "removing";

  return (
    <div className="uninstall-container">
      {error && (
        <div className="uninstall-error">
          {error}
        </div>
      )}

      {phase === "scanning" && <ScanningView />}
      {phase === "list" && !selectedApp && !result && <AppGridView />}
      {phase === "list" && selectedApp && !result && <DetailView />}
      {showRemoving && <RemovingView />}
    </div>
  );
}
