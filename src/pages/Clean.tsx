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
  Package,
  Check,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useCleanStore } from "../stores/cleanStore";
import { checkRunningProcesses, getAppIcon, type RunningApp } from "../lib/tauri";
import { formatSize } from "../utils/format";
import { pickEquivalenceCard, type EquivalenceCard } from "../utils/equivalenceCards";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import BrandIcon, { getBrandIcon } from "../components/BrandIcon";
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

/* ── What gets cleaned chips ── */
const CLEANED_TYPES = [
  { label: "System caches", color: CATEGORY_COLORS["System"] },
  { label: "Browsers", color: CATEGORY_COLORS["Browsers"] },
  { label: "Developer tools", color: CATEGORY_COLORS["Developer Tools"] },
  { label: "App leftovers", color: CATEGORY_COLORS["User"] },
  { label: "AI tools", color: CATEGORY_COLORS["AI Tools"] },
  { label: "Mail & media", color: CATEGORY_COLORS["Email"] },
];

/* ── Idle ── */
function IdleView({ onScan }: { onScan: () => void }) {
  return (
    <div className="clean-centered">
      <div className="clean-idle-icon">
        <Trash2 size={26} strokeWidth={1.5} />
      </div>

      <div className="clean-idle-title">Find reclaimable space</div>
      <div className="clean-idle-desc">
        Scans for system caches, logs, browser data, and app
        leftovers. Typically recovers 2–20 GB.
      </div>

      <button className="btn btn-primary" onClick={onScan}>
        Start Scan
      </button>

      <div className="clean-detected-section">
        <span className="clean-detected-label">WHAT GETS CLEANED</span>
        <div className="clean-detected-types">
          {CLEANED_TYPES.map((t) => (
            <span key={t.label} className="clean-detected-chip">
              <span className="clean-detected-dot" style={{ backgroundColor: t.color }} />
              {t.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

/* ── Scanning ── */
function ScanningView() {
  return (
    <div className="clean-centered">
      <div className="clean-spinner" />
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Scanning files and caches…
      </div>
    </div>
  );
}

/* ── Item icon component ── */
function ItemIcon({ ruleId, appIcon }: { ruleId: string; appIcon?: string }) {
  const size = 18;
  const radius = 4;

  // Real macOS app icon
  if (appIcon) {
    return (
      <img
        src={appIcon}
        alt=""
        style={{
          width: size,
          height: size,
          borderRadius: radius,
          flexShrink: 0,
          objectFit: "contain",
        }}
      />
    );
  }

  // Brand SVG icon for known dev tools/services
  const brandIcon = getBrandIcon(ruleId);
  if (brandIcon) {
    return <BrandIcon ruleId={ruleId} size={size} />;
  }

  // Generic fallback icon
  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        background: "rgba(255, 255, 255, 0.06)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      <Package size={10} color="var(--text-tertiary)" strokeWidth={1.5} />
    </div>
  );
}

/* ── Category row (left panel) ── */
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
  items: { rule_id: string; label: string; total_size: number }[];
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
    <div
      className={`clean-cat-row${isActive ? " active" : ""}`}
      onClick={onClick}
    >
      <input
        type="checkbox"
        className={`checkbox${someSelected ? " partial" : ""}`}
        checked={allSelected}
        onClick={(e) => e.stopPropagation()}
        onChange={() => onToggleCategory(categoryIds, !allSelected)}
      />
      <div className="clean-cat-icon">
        <CatIcon size={14} strokeWidth={1.7} />
      </div>
      <span className="clean-cat-name" ref={containerRef}>
        <span
          ref={nameRef}
          className={`clean-cat-name-inner${isTruncated ? " truncated" : ""}`}
          style={isTruncated ? { "--text-width": `${scrollDist}px` } as React.CSSProperties : undefined}
        >
          {category}
          {isTruncated && (
            <span className="clean-cat-name-dup" aria-hidden="true">{category}</span>
          )}
        </span>
      </span>
      {hasRunning ? (
        <AlertTriangle size={12} strokeWidth={2} className="clean-cat-warning" />
      ) : (
        <span className="clean-cat-warning-spacer" />
      )}
      <span className="clean-cat-size">{formatSize(categorySize)}</span>
    </div>
  );
}

/* ── Category descriptions ── */
const CATEGORY_DESC: Record<string, string> = {
  System: "System caches, logs, and temporary files. Safe to remove — macOS will regenerate them as needed.",
  User: "User-level caches, recent items, and saved state. Removes personalisation data like recent files.",
  Browsers: "Browser caches, cookies, and history. May log you out of websites.",
  "Developer Tools": "IDE caches, package manager stores, and build artifacts. May slow next build or install.",
  Communication: "Chat app caches and downloaded media. Message history is preserved.",
  "AI Tools": "AI assistant caches and local model data. Preferences and accounts are unaffected.",
  Design: "Design tool caches and media previews. Project files remain untouched.",
  "Media & Audio": "Media player caches, thumbnails, and streaming data. Libraries stay intact.",
  "Notes & Productivity": "App caches for notes and productivity tools. Your documents are safe.",
  Utilities: "Utility app caches and plugin data. Settings are preserved.",
  Gaming: "Game launcher caches and shader compilations. Saves and installs are kept.",
  Email: "Email attachment caches and downloaded content. Your mailbox is unaffected.",
  "Saved State": "Window positions and resume data from closed apps. Apps will open fresh.",
  "Orphaned Data": "Leftover data from uninstalled apps. Safe to remove — the parent app no longer exists.",
  Maintenance: "Housekeeping files like .DS_Store. No impact on functionality.",
};

/* ── Detail panel (right panel) ── */
function DetailPanel({
  category,
  items,
  selectedIds,
  onToggle,
  onToggleCategory,
  appIcons,
  runningRuleIds,
}: {
  category: string;
  items: { rule_id: string; label: string; total_size: number }[];
  selectedIds: Set<string>;
  onToggle: (id: string) => void;
  onToggleCategory: (ids: string[], selectAll: boolean) => void;
  appIcons: Record<string, string>;
  runningRuleIds: Set<string>;
}) {
  const visibleItems = items
    .filter((i) => i.total_size > 0)
    .sort((a, b) => b.total_size - a.total_size);
  const selectedInCategory = visibleItems.filter((i) => selectedIds.has(i.rule_id)).length;
  const allSelected = visibleItems.length > 0 && selectedInCategory === visibleItems.length;
  const someSelected = selectedInCategory > 0 && !allSelected;
  const categoryIds = visibleItems.map((i) => i.rule_id);

  const CatIcon = CATEGORY_ICONS[category] || Folder;
  const desc = CATEGORY_DESC[category] || "Cached and temporary data. Safe to remove.";

  return (
    <div className="clean-detail-panel">
      {/* Category info card — outside scroll container */}
      <div className="clean-detail-card">
        <div className="clean-detail-card-icon">
          <CatIcon size={30} strokeWidth={1.3} />
        </div>
        <div className="clean-detail-card-title">{category}</div>
        <div className="clean-detail-card-desc">{desc}</div>
      </div>

      <div className="clean-detail-header">
        <span
          className={`clean-detail-toggle-box${allSelected ? " checked" : ""}${someSelected ? " partial" : ""}`}
          onClick={() => onToggleCategory(categoryIds, !allSelected)}
        >
          {allSelected && (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
              <path d="M2 5L4.5 7.5L8 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          )}
          {someSelected && !allSelected && (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
              <path d="M2.5 5H7.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            </svg>
          )}
        </span>
        <span className="clean-detail-count">
          {selectedInCategory} of {visibleItems.length} items selected
        </span>
      </div>

      <div className="clean-detail-list">
        {visibleItems.map((item) => {
          const isRunning = runningRuleIds.has(item.rule_id);
          return (
            <div key={item.rule_id} className="clean-item" onClick={() => onToggle(item.rule_id)} style={{ cursor: "pointer" }}>
              <input
                type="checkbox"
                className="checkbox"
                checked={selectedIds.has(item.rule_id)}
                onChange={() => onToggle(item.rule_id)}
                onClick={(e) => e.stopPropagation()}
              />
              <ItemIcon ruleId={item.rule_id} appIcon={appIcons[item.rule_id]} />
              <span className="clean-item-label">
                {item.label}
                {isRunning && (
                  <span className="clean-item-running">App is running, deselect or close it to clean</span>
                )}
              </span>
              <span className="clean-item-size">{formatSize(item.total_size)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/* ── Storage visualization bar ── */
function StorageBar({
  categories,
  totalSize,
}: {
  categories: [string, { rule_id: string; label: string; total_size: number }[]][];
  totalSize: number;
}) {
  if (totalSize === 0) return null;

  // Build segments from categories that have non-zero sizes
  const segments = categories
    .map(([name, items]) => {
      const size = items.reduce((s, i) => s + (i.total_size > 0 ? i.total_size : 0), 0);
      return { name, size, color: CATEGORY_COLORS[name] || "#64748b" };
    })
    .filter((s) => s.size > 0);

  return (
    <div className="clean-storage-bar">
      <div className="clean-storage-track">
        {segments.map((seg, i) => {
          const pct = (seg.size / totalSize) * 100;
          return (
            <div
              key={seg.name}
              className="clean-storage-segment"
              style={{
                width: `${Math.max(pct, 0.5)}%`,
                background: `linear-gradient(90deg, color-mix(in srgb, ${seg.color}, white 25%) 0%, ${seg.color} 100%)`,
                borderRadius:
                  i === 0 && i === segments.length - 1
                    ? "4px"
                    : i === 0
                      ? "4px 0 0 4px"
                      : i === segments.length - 1
                        ? "0 4px 4px 0"
                        : "0",
              }}
              title={`${seg.name}: ${formatSize(seg.size)}`}
            />
          );
        })}
      </div>
      <div className="clean-storage-legend">
        {segments.map((seg) => (
          <div key={seg.name} className="clean-storage-legend-item">
            <span
              className="clean-storage-legend-dot"
              style={{ backgroundColor: seg.color }}
            />
            <span className="clean-storage-legend-label">{seg.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ── Results ── */
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

  // Fetch real app icons
  const ruleIds = useMemo(() => items.map((i) => i.rule_id), [items]);
  const appIcons = useAppIcons(ruleIds);

  useEffect(() => {
    checkRunningProcesses(ruleIds).then(setRunningApps).catch(() => {});
  }, [items]);

  const toggleCategory = useCallback(
    (ids: string[], shouldSelect: boolean) => {
      const next = new Set(selectedIds);
      for (const id of ids) {
        if (shouldSelect) {
          next.add(id);
        } else {
          next.delete(id);
        }
      }
      useCleanStore.setState({ selectedIds: next });
    },
    [selectedIds],
  );

  // Group by category (memoized)
  const categories = useMemo(() => {
    const map = new Map<string, { rule_id: string; label: string; total_size: number }[]>();
    for (const item of items) {
      const list = map.get(item.category) || [];
      list.push({ rule_id: item.rule_id, label: item.label, total_size: item.total_size });
      map.set(item.category, list);
    }
    return map;
  }, [items]);

  const { nonZeroItems, selectableIds, totalSize } = useMemo(() => {
    const nz = items.filter((i) => i.total_size > 0);
    return {
      nonZeroItems: nz,
      selectableIds: new Set(nz.map((i) => i.rule_id)),
      totalSize: nz.reduce((sum, i) => sum + i.total_size, 0),
    };
  }, [items]);

  const selectedSize = useMemo(
    () => nonZeroItems.filter((i) => selectedIds.has(i.rule_id)).reduce((sum, i) => sum + i.total_size, 0),
    [nonZeroItems, selectedIds],
  );
  const allSelected = selectableIds.size > 0 && [...selectableIds].every((id) => selectedIds.has(id));

  // Sort categories by size descending (memoized)
  const sortedCategories = useMemo(
    () =>
      Array.from(categories.entries()).sort(
        (a, b) => b[1].reduce((s, i) => s + i.total_size, 0) - a[1].reduce((s, i) => s + i.total_size, 0),
      ),
    [categories],
  );

  // Build set of running rule IDs
  const runningRuleIds = new Set(runningApps.flatMap((a) => a.rule_ids));

  // Empty state
  if (items.length === 0) {
    return (
      <div className="clean-centered">
        <div className="clean-empty-icon">
          <Check size={26} strokeWidth={1.5} />
        </div>
        <div className="clean-empty-title">All clean</div>
        <div className="clean-empty-desc">
          No reclaimable files were found. Your system is already in great shape.
        </div>
        <button className="btn" onClick={() => useCleanStore.getState().scan()} style={{ marginTop: 8 }}>
          Scan Again
        </button>
      </div>
    );
  }

  // Resolve active category — default to first
  const effectiveActive = (activeCategory && categories.has(activeCategory))
    ? activeCategory
    : sortedCategories[0]?.[0] || null;
  const activeItems = effectiveActive
    ? sortedCategories.find(([name]) => name === effectiveActive)?.[1] || []
    : [];

  return (
    <>
      {/* Summary bar */}
      <div className="clean-summary-bar">
        <div className="clean-summary-left">
          <span className="clean-summary-title">Clean</span>
          <span className="clean-summary-size">{formatSize(totalSize)}</span>
          <span className="clean-summary-context">
            items found across {sortedCategories.length} categories
          </span>
        </div>
        <button className="btn" style={{ minWidth: 90 }} onClick={allSelected ? deselectAll : selectAll}>
          {allSelected ? "Deselect All" : "Select All"}
        </button>
      </div>

      {/* Storage visualization */}
      <StorageBar categories={sortedCategories} totalSize={totalSize} />

      {/* Split panel */}
      <div className="clean-split">
        {/* Left: category list */}
        <div className="clean-split-left">
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

        {/* Right: detail panel */}
        <div className="clean-split-right">
          {effectiveActive && (
            <DetailPanel
              key={effectiveActive}
              category={effectiveActive}
              items={activeItems}
              selectedIds={selectedIds}
              onToggle={toggleItem}
              onToggleCategory={toggleCategory}
              appIcons={appIcons}
              runningRuleIds={runningRuleIds}
            />
          )}
        </div>
      </div>

      {/* Footer */}
      <div className="clean-footer">
        <span className="clean-footer-info">
          {selectedIds.size} of {selectableIds.size} items selected
        </span>
        <button
          className="btn btn-primary"
          style={{ minWidth: 120 }}
          disabled={selectedIds.size === 0}
          onClick={() => setShowConfirm(true)}
        >
          Clean {selectedSize > 0 ? formatSize(selectedSize) : ""}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Clean ${selectedIds.size} items (${formatSize(selectedSize)})?`}
        onConfirm={() => { setShowConfirm(false); clean(); }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ── Confetti ── */
const CONFETTI_COLORS = [
  "rgba(255, 255, 255, 0.6)",
  "rgba(255, 255, 255, 0.4)",
  "rgba(255, 255, 255, 0.3)",
  "rgba(253, 72, 65, 0.35)",
  "rgba(42, 200, 82, 0.35)",
  "rgba(58, 123, 255, 0.3)",
  "rgba(253, 210, 37, 0.3)",
  "rgba(142, 92, 246, 0.3)",
];

interface Particle {
  x: number; y: number; vx: number; vy: number;
  rotation: number; rotationSpeed: number;
  size: number; color: string; opacity: number;
  life: number; maxLife: number;
}

function CleanConfetti({ active }: { active: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!active) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    canvas.width = canvas.offsetWidth * 2;
    canvas.height = canvas.offsetHeight * 2;
    ctx.scale(2, 2);

    const w = canvas.offsetWidth;
    const h = canvas.offsetHeight;

    const particles: Particle[] = [];
    for (let i = 0; i < 20; i++) {
      particles.push({
        x: w / 2 + (Math.random() - 0.5) * 60,
        y: h * 0.3,
        vx: (Math.random() - 0.5) * 3,
        vy: -(Math.random() * 2 + 1),
        rotation: Math.random() * 360,
        rotationSpeed: (Math.random() - 0.5) * 8,
        size: Math.random() * 4 + 2,
        color: CONFETTI_COLORS[Math.floor(Math.random() * CONFETTI_COLORS.length)],
        opacity: 1, life: 0,
        maxLife: 1600 + Math.random() * 1800,
      });
    }

    let animId: number;
    let lastTime = performance.now();

    function animate(now: number) {
      const dt = Math.min(now - lastTime, 32);
      lastTime = now;
      ctx!.clearRect(0, 0, w, h);
      let alive = 0;
      for (const p of particles) {
        p.life += dt;
        if (p.life > p.maxLife) continue;
        alive++;
        p.vy += 0.03;
        p.x += p.vx;
        p.y += p.vy;
        p.rotation += p.rotationSpeed;
        p.opacity = 1 - Math.pow(p.life / p.maxLife, 2);
        ctx!.save();
        ctx!.translate(p.x, p.y);
        ctx!.rotate((p.rotation * Math.PI) / 180);
        ctx!.globalAlpha = p.opacity;
        ctx!.fillStyle = p.color;
        ctx!.fillRect(-p.size / 2, -p.size / 2, p.size, p.size * 0.6);
        ctx!.restore();
      }
      if (alive > 0) animId = requestAnimationFrame(animate);
    }

    animId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animId);
  }, [active]);

  if (!active) return null;
  return <canvas ref={canvasRef} className="clean-confetti" />;
}

/* ── SSD Icon for milestone cards ── */
function SsdIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="6" width="18" height="12" rx="2" />
      <line x1="7" y1="10" x2="7" y2="14" />
      <line x1="11" y1="10" x2="11" y2="14" />
      <line x1="15" y1="10" x2="15" y2="14" />
    </svg>
  );
}

/* ── Cleaning / Done View ── */
function CleaningView() {
  const phase = useCleanStore((s) => s.phase);
  const progress = useCleanStore((s) => s.progress);
  const result = useCleanStore((s) => s.result);
  const dismissDone = useCleanStore((s) => s.dismissDone);
  const isDone = phase === "done";

  // Staggered animation state
  const [showCard, setShowCard] = useState(false);
  const [showDoneBtn, setShowDoneBtn] = useState(false);
  const [showConfetti, setShowConfetti] = useState(false);

  // Pick equivalence card once when done
  const cardRef = useRef<EquivalenceCard | null>(null);
  const bytesFreed = isDone && result ? result.bytes_freed : (progress?.bytes_freed || 0);
  if (isDone && !cardRef.current && bytesFreed > 0) {
    cardRef.current = pickEquivalenceCard(bytesFreed);
  }

  useEffect(() => {
    if (!isDone) {
      setShowCard(false);
      setShowDoneBtn(false);
      setShowConfetti(false);
      cardRef.current = null;
      return;
    }
    const t1 = setTimeout(() => { setShowConfetti(true); setShowCard(true); }, 500);
    const t3 = setTimeout(() => setShowDoneBtn(true), 1050);
    return () => { clearTimeout(t1); clearTimeout(t3); };
  }, [isDone]);

  const percent = isDone ? 100
    : progress && progress.items_total > 0
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

  const card = cardRef.current;

  return (
    <div className={`clean-centered${isDone ? " clean-done" : ""}`}>
      <CleanConfetti active={showConfetti} />

      <div className="clean-ring-wrap">
        <svg
          className="clean-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="clean-ring-glass" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.35)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.18)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.30)" />
            </linearGradient>
            <linearGradient id="clean-ring-glass-done" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="rgba(255, 255, 255, 0.5)" />
              <stop offset="50%" stopColor="rgba(255, 255, 255, 0.28)" />
              <stop offset="100%" stopColor="rgba(255, 255, 255, 0.45)" />
            </linearGradient>
            <filter id="clean-ring-glow">
              <feGaussianBlur stdDeviation="3" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none" stroke="rgba(255, 255, 255, 0.06)" strokeWidth={strokeWidth}
          />
          <circle
            cx={ringSize / 2} cy={ringSize / 2} r={radius}
            fill="none"
            stroke={isDone ? "url(#clean-ring-glass-done)" : "url(#clean-ring-glass)"}
            strokeWidth={strokeWidth} strokeLinecap="round"
            strokeDasharray={circumference} strokeDashoffset={dashOffset}
            className="clean-ring-fill"
            filter={isDone ? "url(#clean-ring-glow)" : undefined}
          />
        </svg>
        {isDone ? (
          <Check size={32} strokeWidth={2.5} className="clean-ring-check" />
        ) : (
          <span className="clean-ring-percent">{percent}%</span>
        )}
      </div>

      <div className="clean-ring-freed">
        {isDone && result ? formatSize(result.bytes_freed) : (progress ? formatSize(progress.bytes_freed) : "0 B")} reclaimed
      </div>

      <div className="clean-ring-current">
        {isDone
          ? `${result ? result.items_cleaned : 0} items cleaned`
          : currentLabel}
      </div>

      {isDone && card && (
        <div className={`clean-equiv-card${showCard ? " visible" : ""}`}>
          <div className="clean-equiv-icon">
            {card.isMilestone ? <SsdIcon /> : <span className="clean-equiv-emoji">{card.emoji}</span>}
          </div>
          <div className="clean-equiv-text">
            <div className="clean-equiv-title">{card.title}</div>
            <div className="clean-equiv-desc">{card.description}</div>
          </div>
        </div>
      )}

      {isDone && (
        <button
          className={`btn clean-done-btn${showDoneBtn ? " visible" : ""}`}
          onClick={dismissDone}
        >
          Done
        </button>
      )}
    </div>
  );
}

/* ── Main ── */
export default function Clean() {
  const phase = useCleanStore((s) => s.phase);
  const error = useCleanStore((s) => s.error);
  const scan = useCleanStore((s) => s.scan);

  return (
    <div className="clean-container">
      {error && (
        <div style={{
          fontSize: 12, color: "var(--red)", padding: "8px 12px",
          background: "rgba(253, 72, 65, 0.08)", borderRadius: 6, marginBottom: 10,
        }}>
          {error}
        </div>
      )}

      {phase === "idle" && <IdleView onScan={scan} />}
      {phase === "scanning" && <ScanningView />}
      {phase === "results" && <ResultsView />}
      {(phase === "cleaning" || phase === "done") && <CleaningView />}
    </div>
  );
}
