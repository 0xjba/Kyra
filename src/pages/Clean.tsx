import { useEffect, useState, useCallback, useRef, useMemo } from "react";
import {
  AlertTriangle,
  Monitor,
  User,
  Globe,
  Code,
  MessageCircle,
  Sparkles,
  Palette,
  Music,
  FileText,
  Wrench,
  Gamepad2,
  Mail,
  Archive,
  Folder,
  FolderX,
  Brush,
  Package,
  type LucideIcon,
} from "lucide-react";
import { useCleanStore } from "../stores/cleanStore";
import { askAiClean, canAskAiClean } from "../utils/askAi";
import AskAiCoachMark from "../components/AskAiCoachMark";
import { checkRunningProcesses, getAppIcon, getSystemStats, getTotalBytesFreed, type RunningApp } from "../lib/tauri";
import { formatSize } from "../utils/format";
import { cleanSelection } from "../utils/cleanSelection";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import BrandIcon, { getBrandIcon } from "../components/BrandIcon";
import SuccessOverlay from "../components/SuccessOverlay";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import walk1 from "../assets/cat-walking/cat_walking_01.png";
import walk2 from "../assets/cat-walking/cat_walking_02.png";
import walk3 from "../assets/cat-walking/cat_walking_03.png";
import walk4 from "../assets/cat-walking/cat_walking_04.png";
import walk5 from "../assets/cat-walking/cat_walking_05.png";
import "../styles/clean.css";

/* ── Category colors for storage bar ── */
const CATEGORY_COLORS: Record<string, string> = {
  System: "#f87171",         // red
  User: "#fb923c",           // orange
  Browsers: "#facc15",       // yellow
  "Developer Tools": "#4ade80", // green
  Communication: "#38bdf8",  // sky blue
  "AI Tools": "#a78bfa",     // purple
  Design: "#f472b6",         // pink
  "Media & Audio": "#2dd4bf", // teal
  "Notes & Productivity": "#818cf8", // indigo
  Utilities: "#94a3b8",      // slate
  Gaming: "#e879f9",         // fuchsia
  Email: "#fbbf24",          // amber
  "Saved State": "#64748b",  // gray
  "Orphaned Data": "#fb7185",
  Maintenance: "#a8a29e",
};

/* ── Category icon mapping ── */
const CATEGORY_ICONS: Record<string, LucideIcon> = {
  System: Monitor,
  User: User,
  Browsers: Globe,
  "Developer Tools": Code,
  Communication: MessageCircle,
  "AI Tools": Sparkles,
  Design: Palette,
  "Media & Audio": Music,
  "Notes & Productivity": FileText,
  Utilities: Wrench,
  Gaming: Gamepad2,
  Email: Mail,
  "Saved State": Archive,
  "Orphaned Data": FolderX,
  Maintenance: Brush,
};

/* ── Rule ID → macOS app name (for real icon extraction) ── */
const RULE_APP_NAMES: Record<string, string> = {
  // Browsers
  safari_cache: "Safari",
  chrome_cache: "Google Chrome",
  firefox_cache: "Firefox",
  edge_cache: "Microsoft Edge",
  brave_cache: "Brave Browser",
  arc_cache: "Arc",
  opera_cache: "Opera",
  vivaldi_cache: "Vivaldi",
  orion_cache: "Orion",
  // Communication
  comm_discord: "Discord",
  comm_slack: "Slack",
  comm_zoom: "zoom.us",
  comm_teams: "Microsoft Teams",
  comm_telegram: "Telegram",
  comm_whatsapp: "WhatsApp",
  comm_wechat: "WeChat",
  comm_skype: "Skype",
  comm_signal: "Signal",
  // Media
  media_spotify: "Spotify",
  media_vlc: "VLC",
  media_iina: "IINA",
  media_obs: "OBS",
  media_plex: "Plex",
  media_apple_music: "Music",
  media_apple_tv: "TV",
  media_davinci_resolve: "DaVinci Resolve",
  media_final_cut: "Final Cut Pro",
  media_handbrake: "HandBrake",
  media_podcasts: "Podcasts",
  // Design
  design_figma: "Figma",
  design_sketch: "Sketch",
  design_blender: "Blender",
  // AI Tools
  ai_cursor: "Cursor",
  ai_windsurf: "Windsurf",
  ai_claude_desktop: "Claude",
  ai_chatgpt: "ChatGPT",
  // Notes & Productivity
  notes_notion: "Notion",
  notes_obsidian: "Obsidian",
  notes_evernote: "Evernote",
  notes_bear: "Bear",
  notes_linear: "Linear",
  notes_todoist: "Todoist",
  // Gaming
  game_steam: "Steam",
  game_minecraft: "Minecraft",
  game_epic: "Epic Games Launcher",
  // Email
  email_spark: "Spark",
  email_airmail: "Airmail",
  system_mail_downloads: "Mail",
  // Utilities
  util_homebrew: "Homebrew",
  util_raycast: "Raycast",
  util_alfred: "Alfred 5",
  util_1password: "1Password",
  util_cleanshot: "CleanShot X",
  util_anydesk: "AnyDesk",
  util_teamviewer: "TeamViewer",
  // Dev tools with apps
  dev_vscode_cache: "Visual Studio Code",
  dev_docker_cache: "Docker",
  dev_docker_buildx: "Docker",
};

/* ── Icon cache hook ── */
function useAppIcons(ruleIds: string[]) {
  const [icons, setIcons] = useState<Record<string, string>>({});
  const fetchedRef = useRef(new Set<string>());

  useEffect(() => {
    const toFetch: { ruleId: string; appName: string }[] = [];
    for (const ruleId of ruleIds) {
      const appName = RULE_APP_NAMES[ruleId];
      if (appName && !fetchedRef.current.has(ruleId) && !icons[ruleId]) {
        toFetch.push({ ruleId, appName });
        fetchedRef.current.add(ruleId);
      }
    }
    if (toFetch.length === 0) return;

    // Fetch in parallel, batch update
    Promise.allSettled(
      toFetch.map(async ({ ruleId, appName }) => {
        const icon = await getAppIcon(appName);
        if (icon) return { ruleId, icon };
        return null;
      }),
    ).then((results) => {
      const newIcons: Record<string, string> = {};
      for (const r of results) {
        if (r.status === "fulfilled" && r.value) {
          newIcons[r.value.ruleId] = r.value.icon;
        }
      }
      if (Object.keys(newIcons).length > 0) {
        setIcons((prev) => ({ ...prev, ...newIcons }));
      }
    });
  }, [ruleIds]); // icons intentionally excluded — fetchedRef prevents re-fetching

  return icons;
}

const TAIL_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];
const WALK_FRAMES = [walk1, walk2, walk3, walk4, walk5, walk4, walk3, walk2];
const GB = 1024 * 1024 * 1024;

function useFrame(count: number, ms = 130) {
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setFrame((f) => (f + 1) % count), ms);
    return () => clearInterval(id);
  }, [count, ms]);
  return frame;
}

const colorOf = (category: string) => CATEGORY_COLORS[category] || "#64748b";

const CLEANED_TYPES = [
  { label: "System caches", color: CATEGORY_COLORS["System"] },
  { label: "Browsers", color: CATEGORY_COLORS["Browsers"] },
  { label: "Developer tools", color: CATEGORY_COLORS["Developer Tools"] },
  { label: "App leftovers", color: CATEGORY_COLORS["User"] },
  { label: "AI tools", color: CATEGORY_COLORS["AI Tools"] },
  { label: "Mail & media", color: CATEGORY_COLORS["Email"] },
];

function IdleView({ onScan }: { onScan: () => void }) {
  const frame = useFrame(TAIL_FRAMES.length);
  return (
    <div className="clean-idle">
      <div className="clean-idle-cat-wrap">
        <img src={TAIL_FRAMES[frame]} alt="" className="clean-idle-cat" draggable={false} />
        <div className="clean-idle-bubble">Point me at the mess.</div>
      </div>
      <div className="clean-idle-title">Find reclaimable space</div>
      <div className="clean-idle-desc">
        Scans system caches, logs, browser data and app leftovers. Usually recovers 2 to 20 GB.
      </div>
      <button className="clean-cta" onClick={onScan}>
        Start scan
      </button>
      <div className="clean-detected-section">
        <div className="clean-detected-label">What gets cleaned</div>
        <div className="clean-detected-types">
          {CLEANED_TYPES.map((t) => (
            <span key={t.label} className="clean-detected-chip">
              <span className="clean-detected-dot" style={{ background: t.color }} />
              {t.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

function ScanningView() {
  const frame = useFrame(WALK_FRAMES.length);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    const start = performance.now();
    const id = setInterval(() => setElapsed((performance.now() - start) / 1000), 60);
    return () => clearInterval(id);
  }, []);

  const period = 7;
  const phase = (elapsed % period) / period;
  const forward = phase < 0.5;
  const x = forward ? phase * 2 : 2 - phase * 2;
  const left = 6 + x * 88;
  const segLeft = Math.min(Math.max(left - 12, 0), 76);

  return (
    <div className="clean-scanning">
      <div className="clean-scan-eyebrow">Sniffing around</div>
      <div className="clean-scan-big">Scanning…</div>
      <div className="clean-scan-stage">
        <img
          src={WALK_FRAMES[frame]}
          alt=""
          className="clean-scan-cat"
          draggable={false}
          style={{ left: `${left}%`, transform: forward ? undefined : "scaleX(-1)" }}
        />
        <div className="clean-scan-track">
          <div className="clean-scan-fill" style={{ left: `${segLeft}%` }} />
        </div>
      </div>
      <div className="clean-scan-path">caches · logs · browser data · app leftovers</div>
    </div>
  );
}

function ItemIcon({ ruleId, appIcon }: { ruleId: string; appIcon?: string }) {
  if (appIcon) {
    return <img src={appIcon} alt="" className="clean-item-appicon" />;
  }
  const brandIcon = getBrandIcon(ruleId);
  return (
    <span className="clean-item-icon">
      {brandIcon ? (
        <BrandIcon ruleId={ruleId} size={12} />
      ) : (
        <Package size={12} strokeWidth={2} />
      )}
    </span>
  );
}

type CatItem = { rule_id: string; label: string; total_size: number };

function CategoryRow({
  category,
  items,
  selectedIds,
  onToggleCategory,
  isActive,
  onClick,
  runningRuleIds,
}: {
  category: string;
  items: CatItem[];
  selectedIds: Set<string>;
  onToggleCategory: (ids: string[], selectAll: boolean) => void;
  isActive: boolean;
  onClick: () => void;
  runningRuleIds: Set<string>;
}) {
  const nameRef = useRef<HTMLSpanElement>(null);
  const containerRef = useRef<HTMLSpanElement>(null);
  const [isTruncated, setIsTruncated] = useState(false);
  const [scrollDist, setScrollDist] = useState(0);

  useEffect(() => {
    if (isActive && nameRef.current && containerRef.current) {
      const textW = nameRef.current.scrollWidth;
      const containerW = containerRef.current.clientWidth;
      if (textW > containerW) {
        setIsTruncated(true);
        setScrollDist(textW);
      } else {
        setIsTruncated(false);
      }
    } else {
      setIsTruncated(false);
    }
  }, [isActive, category]);

  const visibleItems = items.filter((i) => i.total_size > 0);
  const categorySize = visibleItems.reduce((sum, i) => sum + i.total_size, 0);
  const selectedInCategory = visibleItems.filter((i) => selectedIds.has(i.rule_id)).length;
  const allSelected = visibleItems.length > 0 && selectedInCategory === visibleItems.length;
  const someSelected = selectedInCategory > 0 && !allSelected;

  if (visibleItems.length === 0) return null;

  const categoryIds = visibleItems.map((i) => i.rule_id);
  const CatIcon = CATEGORY_ICONS[category] || Folder;
  const hasRunning = visibleItems.some((i) => runningRuleIds.has(i.rule_id));

  return (
    <div className={`clean-cat-row${isActive ? " active" : ""}`} onClick={onClick}>
      <input
        type="checkbox"
        className={`checkbox${someSelected ? " partial" : ""}`}
        checked={allSelected}
        onClick={(e) => e.stopPropagation()}
        onChange={() => onToggleCategory(categoryIds, !allSelected)}
      />
      <CatIcon size={15} strokeWidth={2} className="clean-cat-icon" style={{ color: colorOf(category) }} />
      <span className="clean-cat-name" ref={containerRef}>
        <span
          ref={nameRef}
          className={`clean-cat-name-inner${isTruncated ? " truncated" : ""}`}
          style={isTruncated ? ({ "--text-width": `${scrollDist}px` } as React.CSSProperties) : undefined}
        >
          {category}
          {isTruncated && (
            <span className="clean-cat-name-dup" aria-hidden="true">{category}</span>
          )}
        </span>
      </span>
      {hasRunning && <AlertTriangle size={12} strokeWidth={2.2} className="clean-cat-warning" />}
      <span className="clean-cat-size">{formatSize(categorySize)}</span>
    </div>
  );
}

const CATEGORY_DESC: Record<string, string> = {
  System: "System caches, logs and temporary files. Safe to remove; macOS rebuilds them as needed.",
  User: "User-level caches, recent items and saved state.",
  Browsers: "Browser caches, cookies and history. May log you out of websites.",
  "Developer Tools": "IDE caches, package manager stores and build caches. Your next build or install may take a little longer.",
  Communication: "Chat app caches and downloaded media. Message history is kept.",
  "AI Tools": "AI assistant caches and local model data. Preferences and accounts aren't touched.",
  Design: "Design tool caches and media previews. Project files remain untouched.",
  "Media & Audio": "Media player caches, thumbnails and streaming data. Libraries stay intact.",
  "Notes & Productivity": "App caches for notes and productivity tools. Your documents are safe.",
  Utilities: "Utility app caches and plugin data. Settings are preserved.",
  Gaming: "Game launcher caches and shader compilations. Saves and installs are kept.",
  Email: "Attachment caches and downloaded content. Your mailbox is unaffected.",
  "Saved State": "Window positions and resume data from closed apps. Apps will open fresh.",
  "Orphaned Data": "Leftover data from uninstalled apps. Safe to remove; the parent app no longer exists.",
  Maintenance: "Housekeeping files like .DS_Store. No impact on functionality.",
};

function DetailPanel({
  category,
  items,
  selectedIds,
  onToggle,
  appIcons,
  runningRuleIds,
}: {
  category: string;
  items: CatItem[];
  selectedIds: Set<string>;
  onToggle: (id: string) => void;
  appIcons: Record<string, string>;
  runningRuleIds: Set<string>;
}) {
  const visibleItems = items
    .filter((i) => i.total_size > 0)
    .sort((a, b) => b.total_size - a.total_size);
  const categorySize = visibleItems.reduce((sum, i) => sum + i.total_size, 0);
  const CatIcon = CATEGORY_ICONS[category] || Folder;
  const desc = CATEGORY_DESC[category] || "Cached and temporary data. Safe to remove.";
  const color = colorOf(category);

  return (
    <div className="clean-detail">
      <div className="clean-detail-head">
        <div className="clean-detail-icon" style={{ "--cat-color": color } as React.CSSProperties}>
          <CatIcon size={22} strokeWidth={2} style={{ color }} />
        </div>
        <div className="clean-detail-text">
          <div className="clean-detail-titlerow">
            <span className="clean-detail-title">{category}</span>
            <span className="clean-detail-size">{formatSize(categorySize)}</span>
          </div>
          <div className="clean-detail-desc">{desc}</div>
        </div>
      </div>
      <div className="clean-detail-divider" />
      <div className="clean-detail-list">
        {visibleItems.map((item) => {
          const isRunning = runningRuleIds.has(item.rule_id);
          return (
            <div key={item.rule_id} className="clean-item" onClick={() => onToggle(item.rule_id)}>
              <input
                type="checkbox"
                className="checkbox"
                checked={selectedIds.has(item.rule_id)}
                onChange={() => onToggle(item.rule_id)}
                onClick={(e) => e.stopPropagation()}
              />
              <ItemIcon ruleId={item.rule_id} appIcon={appIcons[item.rule_id]} />
              <div className="clean-item-text">
                <span className="clean-item-label">{item.label}</span>
                {isRunning && (
                  <span className="clean-item-running">App is running. Close it or leave it unchecked.</span>
                )}
              </div>
              <span className="clean-item-size">{formatSize(item.total_size)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function StorageBar({
  categories,
  selectedIds,
  scannedSize,
  diskTotal,
  diskFree,
}: {
  categories: [string, CatItem[]][];
  selectedIds: Set<string>;
  scannedSize: number;
  diskTotal: number;
  diskFree: number;
}) {
  const segments = categories
    .map(([name, items]) => {
      const visible = items.filter((i) => i.total_size > 0);
      const size = visible.reduce((s, i) => s + i.total_size, 0);
      const anySelected = visible.some((i) => selectedIds.has(i.rule_id));
      return { name, size, color: colorOf(name), anySelected };
    })
    .filter((s) => s.size > 0);

  const hasDisk = diskTotal > 0;
  const diskUsed = Math.max(0, diskTotal - diskFree);
  const otherSize = Math.max(0, diskUsed - scannedSize);

  return (
    <div className="clean-storage">
      <div className="clean-storage-track">
        {segments.map((seg) => (
          <span
            key={seg.name}
            className="clean-storage-seg"
            title={`${seg.name}: ${formatSize(seg.size)}`}
            style={{
              flex: seg.size,
              background: `linear-gradient(180deg, rgba(255,255,255,0.35), rgba(255,255,255,0) 60%), ${seg.color}`,
              opacity: seg.anySelected ? 1 : 0.3,
            }}
          />
        ))}
        {hasDisk && otherSize > 0 && (
          <span className="clean-storage-other" style={{ flex: otherSize }} title={`Other: ${formatSize(otherSize)}`} />
        )}
        {hasDisk && diskFree > 0 && (
          <span className="clean-storage-free" style={{ flex: diskFree }} title={`Free: ${formatSize(diskFree)}`} />
        )}
      </div>
      <div className="clean-storage-meta">
        <span>Startup disk · {formatSize(scannedSize)} reclaimable</span>
        {hasDisk && (
          <span>
            {Math.round(diskUsed / GB)} of {Math.round(diskTotal / GB)} GB used
          </span>
        )}
      </div>
    </div>
  );
}

function ResultsView() {
  const items = useCleanStore((s) => s.items);
  const selectedIds = useCleanStore((s) => s.selectedIds);
  const toggleItem = useCleanStore((s) => s.toggleItem);
  const selectAll = useCleanStore((s) => s.selectAll);
  const deselectAll = useCleanStore((s) => s.deselectAll);
  const clean = useCleanStore((s) => s.clean);

  const [runningApps, setRunningApps] = useState<RunningApp[]>([]);
  const [showConfirm, setShowConfirm] = useState(false);
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [diskTotal, setDiskTotal] = useState(0);
  const [diskFree, setDiskFree] = useState(0);

  const ruleIds = useMemo(() => items.map((i) => i.rule_id), [items]);
  const appIcons = useAppIcons(ruleIds);

  useEffect(() => {
    checkRunningProcesses(ruleIds).then(setRunningApps).catch(() => {});
    getSystemStats().then((s) => { setDiskTotal(s.disk_total); setDiskFree(s.disk_free); }).catch(() => {});
  }, [items]);

  const toggleCategory = useCallback(
    (ids: string[], shouldSelect: boolean) => {
      const next = new Set(selectedIds);
      for (const id of ids) {
        if (shouldSelect) next.add(id);
        else next.delete(id);
      }
      useCleanStore.setState({ selectedIds: next });
    },
    [selectedIds],
  );

  const categories = useMemo(() => {
    const map = new Map<string, CatItem[]>();
    for (const item of items) {
      const list = map.get(item.category) || [];
      list.push({ rule_id: item.rule_id, label: item.label, total_size: item.total_size });
      map.set(item.category, list);
    }
    return map;
  }, [items]);

  const { nonZeroItems, selectableIds, totalSize, selectedSize, selectedCount, allSelected } = useMemo(
    () => cleanSelection(items, selectedIds),
    [items, selectedIds],
  );

  const sortedCategories = useMemo(
    () =>
      Array.from(categories.entries())
        .filter(([, list]) => list.some((i) => i.total_size > 0))
        .sort(
          (a, b) => b[1].reduce((s, i) => s + i.total_size, 0) - a[1].reduce((s, i) => s + i.total_size, 0),
        ),
    [categories],
  );

  const runningRuleIds = new Set(runningApps.flatMap((a) => a.rule_ids));

  if (items.length === 0 || nonZeroItems.length === 0) {
    return (
      <div className="clean-idle">
        <img src={cat1} alt="" className="clean-empty-cat" draggable={false} />
        <div className="clean-idle-title">All clean</div>
        <div className="clean-idle-desc">
          No reclaimable files were found. Your system is already in great shape.
        </div>
        <button className="clean-cta" onClick={() => useCleanStore.getState().scan()}>
          Scan again
        </button>
      </div>
    );
  }

  const effectiveActive = (activeCategory && categories.has(activeCategory))
    ? activeCategory
    : sortedCategories[0]?.[0] || null;
  const activeItems = effectiveActive
    ? sortedCategories.find(([name]) => name === effectiveActive)?.[1] || []
    : [];

  const askDisabled = selectedIds.size === 0 || !canAskAiClean(items, selectedIds);

  return (
    <div className="clean-results">
      <div className="clean-results-head">
        <div className="clean-results-total">
          <span className="clean-results-size">{formatSize(totalSize)}</span>
          <span className="clean-results-context">
            found across {sortedCategories.length} {sortedCategories.length === 1 ? "category" : "categories"}
          </span>
        </div>
        <button className="clean-pill" onClick={allSelected ? deselectAll : selectAll}>
          {allSelected ? "Deselect all" : "Select all"}
        </button>
      </div>

      <StorageBar
        categories={sortedCategories}
        selectedIds={selectedIds}
        scannedSize={totalSize}
        diskTotal={diskTotal}
        diskFree={diskFree}
      />

      <div className="clean-split">
        <div className="clean-panel clean-cat-list">
          {sortedCategories.map(([category, categoryItems]) => (
            <CategoryRow
              key={category}
              category={category}
              items={categoryItems}
              selectedIds={selectedIds}
              onToggleCategory={toggleCategory}
              isActive={effectiveActive === category}
              onClick={() => setActiveCategory(category)}
              runningRuleIds={runningRuleIds}
            />
          ))}
        </div>
        <div className="clean-panel clean-detail-panel">
          {effectiveActive && (
            <DetailPanel
              key={effectiveActive}
              category={effectiveActive}
              items={activeItems}
              selectedIds={selectedIds}
              onToggle={toggleItem}
              appIcons={appIcons}
              runningRuleIds={runningRuleIds}
            />
          )}
        </div>
      </div>

      <div className="clean-footer">
        <span className="clean-footer-info">
          {selectedCount} of {selectableIds.size} items selected
        </span>
        <span
          className="tooltip-wrap clean-footer-ask"
          data-tooltip={selectedIds.size > 0 && !canAskAiClean(items, selectedIds) ? "Select fewer items to Ask AI" : undefined}
        >
          <AskAiCoachMark />
          <button
            className="clean-pill clean-ask-btn"
            disabled={askDisabled}
            onClick={() => askAiClean(items, selectedIds)}
          >
            <Sparkles size={13} strokeWidth={2.2} className="clean-ask-icon" />
            Ask AI
          </button>
        </span>
        <button
          className="clean-action-btn"
          disabled={selectedIds.size === 0}
          onClick={() => setShowConfirm(true)}
        >
          Clean {formatSize(selectedSize)}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Clean ${selectedCount} items (${formatSize(selectedSize)})?`}
        onConfirm={() => { setShowConfirm(false); clean(); }}
        onCancel={() => setShowConfirm(false)}
      />
    </div>
  );
}

function CleaningProgressView() {
  const progress = useCleanStore((s) => s.progress);

  const percent = progress && progress.paths_total > 0
    ? Math.round((progress.paths_done / progress.paths_total) * 100)
    : 0;

  const circumference = 2 * Math.PI * 68;
  const dashOffset = circumference * (1 - percent / 100);

  const currentName = progress?.current_item
    ? progress.current_item.split("/").filter(Boolean).pop() || progress.current_item
    : null;

  return (
    <div className="clean-working">
      <div className="clean-ring">
        <svg width="150" height="150" viewBox="0 0 150 150" className="clean-ring-svg">
          <circle cx="75" cy="75" r="68" fill="none" strokeWidth="10" className="clean-ring-track" />
          <circle
            cx="75" cy="75" r="68"
            fill="none"
            stroke="#1f5fff"
            strokeWidth="10"
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            className="clean-ring-fill"
          />
        </svg>
        <div className="clean-ring-center">
          <span className="clean-ring-percent">{percent}%</span>
        </div>
      </div>
      <div className="clean-ring-freed">
        {progress ? formatSize(progress.bytes_freed) : "0 B"}&nbsp;<span>reclaimed</span>
      </div>
      <div className="clean-ring-current">
        {currentName ? `Removing ${currentName}…` : "Starting…"}
      </div>
    </div>
  );
}

function CleanDoneView() {
  const result = useCleanStore((s) => s.result);
  const items = useCleanStore((s) => s.items);
  const dismissDone = useCleanStore((s) => s.dismissDone);

  const [diskTotal, setDiskTotal] = useState(0);
  const [diskFree, setDiskFree] = useState(0);
  const [lifetimeBytes, setLifetimeBytes] = useState(0);

  useEffect(() => {
    getSystemStats()
      .then((s) => { setDiskTotal(s.disk_total); setDiskFree(s.disk_free); })
      .catch(() => {});
    getTotalBytesFreed()
      .then(setLifetimeBytes)
      .catch(() => {});
  }, []);

  const bytesFreed = result?.bytes_freed ?? 0;
  const issues = [...(result?.failed ?? []), ...(result?.already_gone ?? [])];
  const itemCount = result?.items_cleaned ?? 0;
  const cleanedIds = new Set(result?.cleaned_ids ?? []);
  const categoryCount = new Set(
    items.filter((i) => cleanedIds.has(i.rule_id)).map((i) => i.category),
  ).size;

  return (
    <SuccessOverlay
      headline="All clean"
      freedGB={bytesFreed / GB}
      detail="back on your Mac"
      itemCount={itemCount}
      categoryCount={categoryCount}
      lifetimeGB={lifetimeBytes / GB}
      storageUsedGB={(diskTotal - diskFree) / GB}
      storageTotalGB={diskTotal / GB}
      showPawtrolUpsell={true}
      failedBytes={result?.bytes_failed ?? 0}
      issues={issues}
      onDone={dismissDone}
    />
  );
}

export default function Clean() {
  const phase = useCleanStore((s) => s.phase);
  const error = useCleanStore((s) => s.error);
  const scan = useCleanStore((s) => s.scan);

  return (
    <div className="clean-container">
      {error && <div className="clean-error">{error}</div>}
      {phase === "idle" && <IdleView onScan={scan} />}
      {phase === "scanning" && <ScanningView />}
      {phase === "results" && <ResultsView />}
      {phase === "cleaning" && <CleaningProgressView />}
      {phase === "done" && <CleanDoneView />}
    </div>
  );
}
