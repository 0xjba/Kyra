import { useEffect, useMemo, useState } from "react";
import {
  CornerLeftUp,
  Disc,
  File,
  FileArchive,
  FileImage,
  FileMusic,
  FileVideoCamera,
  FolderOpen,
  Package,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useAnalyzeStore } from "../stores/analyzeStore";
import { useSettingsStore } from "../stores/settingsStore";
import { addBytesFreed, deleteAnalyzedItem, isProtectedDataRefusal, pickFolder, type DirNode, type LargeFile } from "../lib/tauri";
import Treemap, { type TreemapItem } from "../components/Treemap";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import { formatSize } from "../utils/format";
import catImg from "../assets/cat.png";
import "../styles/analyze.css";

const HUES = ["#22B8F0", "#2AC852", "#FDD225", "#FD8C34", "#FD4841", "#8E5CF6", "#3A7BFF"];

const PROTECTED = ["/System", "/bin", "/sbin", "/usr", "/etc", "/var", "/private", "/Applications", "/Library"];

function isProtected(path: string): boolean {
  if (path === "/" || path === "/Users") return true;
  if (/^\/Users\/[^/]+\/?$/.test(path)) return true;
  return PROTECTED.some((p) => path === p || path.startsWith(p + "/"));
}

function rootLabel(path: string, name: string): string {
  if (path === "/" || name === "/" || name === "") return "Macintosh HD";
  return name;
}

function sortedChildren(node: DirNode): DirNode[] {
  return [...node.children].sort((a, b) => b.size - a.size);
}

function share(part: number, whole: number): string {
  return `${whole > 0 ? Math.round((part / whole) * 100) : 0}%`;
}

function thresholdLabel(mb: number): string {
  return mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`;
}

function shortenHome(path: string): string {
  return path.replace(/^\/Users\/[^/]+(?=\/|$)/, "~");
}

function parentDir(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx > 0 ? path.slice(0, idx) : "/";
}

const FILE_KINDS: [LucideIcon, string[]][] = [
  [FileVideoCamera, ["mp4", "mov", "m4v", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg", "prores"]],
  [FileMusic, ["mp3", "wav", "aiff", "aif", "flac", "m4a", "aac", "ogg", "caf", "logicx"]],
  [FileImage, ["jpg", "jpeg", "png", "heic", "tif", "tiff", "psd", "raw", "cr2", "nef", "dng", "gif", "exr"]],
  [FileArchive, ["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "zst"]],
  [Disc, ["dmg", "iso", "img", "sparseimage", "sparsebundle", "vmdk", "vdi", "qcow2", "hdd"]],
  [Package, ["pkg", "mpkg", "ipa", "xip", "apk"]],
];

function fileIcon(name: string): LucideIcon {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return File;
  const ext = name.slice(dot + 1).toLowerCase();
  return FILE_KINDS.find(([, exts]) => exts.includes(ext))?.[0] ?? File;
}

function buildLargeTip(files: LargeFile[], total: number, threshold: string): string {
  if (files.length === 0) return `Nothing over ${threshold} here.`;
  const top = files[0];
  if (files.length === 1) return `${top.name} is the only file over ${threshold}, at ${formatSize(top.size)}.`;
  const pct = total > 0 ? Math.round((top.size / total) * 100) : 0;
  if (pct >= 50) return `${top.name} is ${pct}% of this list all by itself.`;
  const second = files[1];
  return `${top.name} is the biggest at ${formatSize(top.size)}, ${pct}% of this list. ${second.name} is next at ${formatSize(second.size)}.`;
}

function buildTip(current: DirNode, atDiskRoot: boolean): string {
  const kids = sortedChildren(current).filter((c) => c.size > 0);
  if (kids.length === 0) return "Nothing in here takes up space.";
  const top = kids[0];
  const where = atDiskRoot ? "your disk" : "this folder";
  const pct = current.size > 0 ? Math.round((top.size / current.size) * 100) : 0;
  const first = pct >= 50 ? `${top.name} is most of ${where}.` : `${top.name} is the biggest thing in ${where} at ${pct}%.`;
  const inner = top.is_dir ? sortedChildren(top).filter((c) => c.size > 0)[0] : undefined;
  if (inner) {
    const label = inner.is_dir ? `Its ${inner.name} folder` : inner.name;
    return `${first} ${label} alone is ${formatSize(inner.size)}.`;
  }
  if (kids.some((c) => c.is_dir && c.children.length > 0)) return `${first} Click any folder to dig deeper.`;
  return first;
}

function ViewToggle() {
  const activeTab = useAnalyzeStore((s) => s.activeTab);
  const setActiveTab = useAnalyzeStore((s) => s.setActiveTab);
  const opts = [
    { id: "tree" as const, label: "Disk map" },
    { id: "large-files" as const, label: "Large files" },
  ];
  return (
    <div className="analyze-seg" role="tablist">
      {opts.map((o) => (
        <button
          key={o.id}
          role="tab"
          aria-selected={activeTab === o.id}
          className={`analyze-seg-opt${activeTab === o.id ? " on" : ""}`}
          onClick={() => setActiveTab(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function TopBar({ scanning }: { scanning: boolean }) {
  const activeTab = useAnalyzeStore((s) => s.activeTab);
  const largeFilesLoading = useAnalyzeStore((s) => s.largeFilesLoading);
  const root = useAnalyzeStore((s) => s.root);
  const current = useAnalyzeStore((s) => s.current);
  const breadcrumb = useAnalyzeStore((s) => s.breadcrumb);
  const scanPath = useAnalyzeStore((s) => s.scanPath);
  const drillUp = useAnalyzeStore((s) => s.drillUp);
  const drillToRoot = useAnalyzeStore((s) => s.drillToRoot);
  const drillToIndex = useAnalyzeStore((s) => s.drillToIndex);
  const setScanPath = useAnalyzeStore((s) => s.setScanPath);
  const scan = useAnalyzeStore((s) => s.scan);

  const largeMode = activeTab === "large-files";
  const crumbs: { name: string; open?: () => void }[] = [];
  if (current && root && !largeMode) {
    const trail = [...breadcrumb, current];
    trail.forEach((n, i) => {
      const last = i === trail.length - 1;
      const name = i === 0 ? rootLabel(n.path, n.name) : n.name;
      const open = last ? undefined : i === 0 ? drillToRoot : () => drillToIndex(i);
      crumbs.push({ name, open });
    });
  } else {
    const name = scanPath.split("/").filter(Boolean).pop() ?? "";
    crumbs.push({ name: rootLabel(scanPath, name) });
  }
  const shownCrumbs =
    crumbs.length > 4 ? [crumbs[0], { name: "…" }, ...crumbs.slice(-2)] : crumbs;

  const meta = largeMode
    ? ""
    : current
    ? `${formatSize(current.size)} · ${current.children.length} items`
    : scanning
      ? "Scanning…"
      : "";

  const canUp = breadcrumb.length > 0;

  const handlePick = async () => {
    const selected = await pickFolder();
    if (selected) {
      setScanPath(selected);
      scan();
    }
  };

  return (
    <div className="analyze-topbar">
      <div className="analyze-crumbs">
        {shownCrumbs.map((c, i) => (
          <button
            key={i}
            className={`analyze-crumb${i === shownCrumbs.length - 1 ? " current" : ""}`}
            onClick={c.open}
            disabled={!c.open}
          >
            {c.name}
          </button>
        ))}
      </div>
      {meta && <span className="analyze-topbar-meta">{meta}</span>}
      <ViewToggle />
      <button
        className="analyze-up-btn analyze-pick-btn"
        onClick={handlePick}
        disabled={scanning || (largeMode && largeFilesLoading)}
        title="Choose a folder to analyze"
      >
        <FolderOpen size={13} strokeWidth={2} />
      </button>
      {!largeMode && (
        <button className="analyze-up-btn" onClick={drillUp} disabled={!canUp} style={{ opacity: canUp ? 1 : 0.4 }}>
          <CornerLeftUp size={13} strokeWidth={2} />
          Up
        </button>
      )}
    </div>
  );
}

function CatTip({ text }: { text: string }) {
  return (
    <div className="analyze-cat-row">
      <img src={catImg} alt="" className="analyze-cat" />
      <div className="analyze-cat-bubble">{text}</div>
    </div>
  );
}

function ScanningPanel() {
  const progress = useAnalyzeStore((s) => s.progress);
  const scanPath = useAnalyzeStore((s) => s.scanPath);
  const name = rootLabel(scanPath, scanPath.split("/").filter(Boolean).pop() ?? "");
  return (
    <div className="analyze-map-panel">
      <div className="analyze-map-state">
        <div className="spinner" />
        <div className="analyze-state-title">Scanning {name}…</div>
        <div className="analyze-state-sub">
          {progress ? `${progress.files_scanned.toLocaleString()} files scanned` : "Getting started"}
        </div>
      </div>
    </div>
  );
}

function ScanningSidebar() {
  const progress = useAnalyzeStore((s) => s.progress);
  const scanPath = useAnalyzeStore((s) => s.scanPath);
  const name = rootLabel(scanPath, scanPath.split("/").filter(Boolean).pop() ?? "");
  return (
    <div className="analyze-sidebar">
      <div className="analyze-sidebar-header">
        <div className="analyze-sidebar-section-label">Scanning</div>
        <div className="analyze-sidebar-title">{name}</div>
        <div className="analyze-sidebar-size-row">
          <span className="analyze-sidebar-size-big">{formatSize(progress?.total_size ?? 0)}</span>
          <span className="analyze-sidebar-size-share">found so far</span>
        </div>
        <div className="analyze-sidebar-path">{progress?.current_path || scanPath}</div>
      </div>
      <div className="analyze-sidebar-list" />
      <CatTip text="Measuring every folder. Big disks can take a minute." />
    </div>
  );
}

function ErrorPanel() {
  const error = useAnalyzeStore((s) => s.error);
  const scan = useAnalyzeStore((s) => s.scan);
  const setScanPath = useAnalyzeStore((s) => s.setScanPath);
  const handlePick = async () => {
    const selected = await pickFolder();
    if (selected) {
      setScanPath(selected);
      scan();
    }
  };
  return (
    <>
      <div className="analyze-map-panel">
        <div className="analyze-map-state">
          <div className="analyze-state-title">Couldn't finish the scan</div>
          <div className="analyze-state-sub analyze-state-error">{error}</div>
          <div className="analyze-state-actions">
            <button className="btn" onClick={handlePick}>Choose Folder</button>
            <button className="btn btn-primary" onClick={() => scan()}>Try Again</button>
          </div>
        </div>
      </div>
      <div className="analyze-sidebar">
        <div className="analyze-sidebar-list" />
        <CatTip text="That didn't work. Try again, or pick a different folder." />
      </div>
    </>
  );
}

function ReadyView() {
  const root = useAnalyzeStore((s) => s.root);
  const current = useAnalyzeStore((s) => s.current);
  const breadcrumb = useAnalyzeStore((s) => s.breadcrumb);
  const drillInto = useAnalyzeStore((s) => s.drillInto);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const removeNodeByPath = useAnalyzeStore((s) => s.removeNodeByPath);
  const removeLargeFile = useAnalyzeStore((s) => s.removeLargeFile);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [hoveredPath, setHoveredPath] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<DirNode | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    setHoveredPath(null);
    setNotice(null);
  }, [current?.path]);

  const kids = useMemo(() => (current ? sortedChildren(current) : []), [current]);

  const baseHue = useMemo(() => {
    if (!root || breadcrumb.length === 0 || !current) return null;
    const top = breadcrumb.length >= 2 ? breadcrumb[1] : current;
    const idx = sortedChildren(root).findIndex((c) => c.path === top.path);
    return HUES[Math.max(0, idx) % HUES.length];
  }, [root, breadcrumb, current]);

  const items: TreemapItem[] = useMemo(
    () =>
      kids.map((node, rank) => ({
        node,
        fill: baseHue
          ? `color-mix(in srgb, ${baseHue} ${Math.max(28, 70 - rank * 10)}%, white)`
          : `color-mix(in srgb, ${HUES[rank % HUES.length]} 62%, white)`,
      })),
    [kids, baseHue],
  );

  if (!current || !root) return null;

  const hovered = kids.find((k) => k.path === hoveredPath) ?? kids[0] ?? null;
  const atDiskRoot = breadcrumb.length === 0 && root.path === "/";
  const tip = notice ?? buildTip(current, atDiskRoot);
  const trashDisabled = !hovered || deleting || isProtected(hovered.path);

  const [browserTarget, setBrowserTarget] = useState<DirNode | null>(null);

  const runDelete = async (target: DirNode, allowBrowserData: boolean) => {
    setDeleting(true);
    try {
      const freed = await deleteAnalyzedItem(target.path, !useTrash, allowBrowserData);
      removeNodeByPath(target.path, target.size);
      removeLargeFile(target.path);
      if (freed > 0) addBytesFreed(freed).catch(() => {});
      setHoveredPath(null);
      setNotice(`${useTrash ? "Moved" : "Deleted"} ${target.name}${useTrash ? " to Trash" : ""}. That's ${formatSize(freed || target.size)} back.`);
    } catch (e) {
      if (!allowBrowserData && isProtectedDataRefusal(e)) setBrowserTarget(target);
      else useAnalyzeStore.setState({ error: String(e) });
    } finally {
      setDeleting(false);
    }
  };

  const handleDelete = async () => {
    const target = deleteTarget;
    setDeleteTarget(null);
    if (!target || deleting) return;
    await runDelete(target, false);
  };

  const handleBrowserConfirm = async () => {
    const target = browserTarget;
    setBrowserTarget(null);
    if (target) await runDelete(target, true);
  };

  return (
    <>
      <div className="analyze-map-panel">
        <Treemap
          items={items}
          animKey={current.path}
          hoveredPath={hovered?.path ?? null}
          onDrillIn={drillInto}
          onHover={(n) => setHoveredPath(n.path)}
        />
      </div>

      <div className="analyze-sidebar">
        {hovered ? (
          <div className="analyze-sidebar-header">
            <div className="analyze-sidebar-section-label">Hovering</div>
            <div className="analyze-sidebar-title">{hovered.name}</div>
            <div className="analyze-sidebar-size-row">
              <span className="analyze-sidebar-size-big">{formatSize(hovered.size)}</span>
              <span className="analyze-sidebar-size-share">{share(hovered.size, current.size)} of this folder</span>
            </div>
            <div className="analyze-sidebar-path" title={hovered.path}>{hovered.path}</div>
            <div className="analyze-sidebar-actions">
              <button className="analyze-sidebar-reveal-btn" onClick={() => reveal(hovered.path)}>
                Reveal in Finder
              </button>
              <button
                className="analyze-sidebar-trash-btn"
                onClick={() => setDeleteTarget(hovered)}
                disabled={trashDisabled}
                title={isProtected(hovered.path) ? "Protected location" : useTrash ? "Move to Trash" : "Delete"}
              >
                <Trash2 size={13} strokeWidth={2} />
              </button>
            </div>
          </div>
        ) : (
          <div className="analyze-sidebar-header">
            <div className="analyze-sidebar-section-label">Hovering</div>
            <div className="analyze-sidebar-title">Nothing here</div>
          </div>
        )}
        <div className="analyze-sidebar-divider" />
        <div className="analyze-sidebar-list">
          {items.filter(({ node }) => node.size > 0).map(({ node, fill }) => {
            const canDrill = node.is_dir && node.children.length > 0;
            return (
              <div
                key={node.path}
                className={`analyze-sidebar-list-row${canDrill ? " analyze-sidebar-list-drillable" : ""}`}
                onClick={() => canDrill && drillInto(node)}
                onMouseEnter={() => setHoveredPath(node.path)}
              >
                <span className="analyze-sidebar-list-dot" style={{ background: fill }} />
                <span className="analyze-sidebar-list-name">{node.name}</span>
                <span className="analyze-sidebar-list-size">{formatSize(node.size)}</span>
              </div>
            );
          })}
        </div>
        <CatTip text={tip} />
      </div>

      <DeleteConfirmDialog
        visible={deleteTarget !== null}
        title={`Delete "${deleteTarget?.name ?? ""}" (${formatSize(deleteTarget?.size ?? 0)})?`}
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
      <DeleteConfirmDialog
        visible={browserTarget !== null}
        title="This is personal data"
        description={`"${browserTarget?.name ?? ""}" holds data that can't be downloaded again, like a browser profile, wallet, message history or backup.${useTrash ? " It goes to the Trash, so you can still restore it." : " This can't be undone."}`}
        confirmLabel="Delete anyway"
        destructive
        onConfirm={handleBrowserConfirm}
        onCancel={() => setBrowserTarget(null)}
      />
    </>
  );
}

function LargeFilesView() {
  const files = useAnalyzeStore((s) => s.largeFiles);
  const loading = useAnalyzeStore((s) => s.largeFilesLoading);
  const listKey = useAnalyzeStore((s) => s.largeFilesKey);
  const capped = useAnalyzeStore((s) => s.largeFilesCapped);
  const reveal = useAnalyzeStore((s) => s.reveal);
  const removeLargeFile = useAnalyzeStore((s) => s.removeLargeFile);
  const removeNodeByPath = useAnalyzeStore((s) => s.removeNodeByPath);
  const thresholdMb = useSettingsStore((s) => s.settings.large_file_threshold_mb) || 100;
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  const [hoveredPath, setHoveredPath] = useState<string | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<LargeFile | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    setHoveredPath(null);
    setSelectedPath(null);
    setNotice(null);
  }, [listKey]);

  const threshold = thresholdLabel(thresholdMb);
  const total = useMemo(() => files.reduce((sum, f) => sum + f.size, 0), [files]);

  const hovered = files.find((f) => f.path === hoveredPath);
  const selected = files.find((f) => f.path === selectedPath);
  const shown = hovered ?? selected ?? files[0] ?? null;
  const shownLabel = hovered ? "Hovering" : selected ? "Selected" : "Biggest";
  const busy = loading || listKey === null;
  const tip = busy
    ? "Asking Spotlight for the heavyweights. Won't take long."
    : notice ?? buildLargeTip(files, total, threshold);
  const trashDisabled = !shown || deleting || isProtected(shown.path);

  const count = files.length;
  const heading = capped
    ? `Top ${count} file${count === 1 ? "" : "s"} over ${threshold}`
    : `${count} file${count === 1 ? "" : "s"} over ${threshold}`;

  const [browserTarget, setBrowserTarget] = useState<typeof deleteTarget>(null);

  const runDelete = async (target: NonNullable<typeof deleteTarget>, allowBrowserData: boolean) => {
    setDeleting(true);
    try {
      const freed = await deleteAnalyzedItem(target.path, !useTrash, allowBrowserData);
      removeLargeFile(target.path);
      removeNodeByPath(target.path, target.size);
      if (freed > 0) addBytesFreed(freed).catch(() => {});
      setHoveredPath(null);
      setSelectedPath(null);
      setNotice(`${useTrash ? "Moved" : "Deleted"} ${target.name}${useTrash ? " to Trash" : ""}. That's ${formatSize(freed || target.size)} back.`);
    } catch (e) {
      if (!allowBrowserData && isProtectedDataRefusal(e)) setBrowserTarget(target);
      else setNotice(`Couldn't ${useTrash ? "move" : "delete"} ${target.name}. ${String(e)}`);
    } finally {
      setDeleting(false);
    }
  };

  const handleDelete = async () => {
    const target = deleteTarget;
    setDeleteTarget(null);
    if (!target || deleting) return;
    await runDelete(target, false);
  };

  const handleBrowserConfirm = async () => {
    const target = browserTarget;
    setBrowserTarget(null);
    if (target) await runDelete(target, true);
  };

  return (
    <>
      <div className="analyze-map-panel analyze-files-panel">
        {busy ? (
          <div className="analyze-map-state">
            <div className="spinner" />
            <div className="analyze-state-title">Finding files over {threshold}…</div>
          </div>
        ) : count === 0 ? (
          <div className="analyze-map-state">
            <div className="analyze-state-title">Nothing over {threshold} here.</div>
            <div className="analyze-state-sub">Lower the threshold in Settings to see smaller files.</div>
          </div>
        ) : (
          <>
            <div className="analyze-files-header">
              <span className="analyze-files-heading">{heading}</span>
              <span className="analyze-files-total">{formatSize(total)}</span>
            </div>
            <div className="analyze-files-list" onMouseLeave={() => setHoveredPath(null)}>
              {files.map((f) => {
                const Icon = fileIcon(f.name);
                const active = shown?.path === f.path;
                return (
                  <div
                    key={f.path}
                    className={`analyze-file-row${active ? " active" : ""}`}
                    onMouseEnter={() => setHoveredPath(f.path)}
                    onClick={() => setSelectedPath(f.path)}
                    onDoubleClick={() => reveal(f.path)}
                  >
                    <span className="analyze-file-icon">
                      <Icon size={16} strokeWidth={1.75} />
                    </span>
                    <div className="analyze-file-text">
                      <div className="analyze-file-name">{f.name}</div>
                      <div className="analyze-file-path" title={f.path}>{shortenHome(parentDir(f.path))}</div>
                    </div>
                    <span className="analyze-file-size">{formatSize(f.size)}</span>
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>

      <div className="analyze-sidebar">
        {shown && !busy ? (
          <div className="analyze-sidebar-header">
            <div className="analyze-sidebar-section-label">{shownLabel}</div>
            <div className="analyze-sidebar-title" title={shown.name}>{shown.name}</div>
            <div className="analyze-sidebar-size-row">
              <span className="analyze-sidebar-size-big">{formatSize(shown.size)}</span>
              <span className="analyze-sidebar-size-share">{share(shown.size, total)} of this list</span>
            </div>
            <div className="analyze-sidebar-path" title={shown.path}>{shortenHome(shown.path)}</div>
            <div className="analyze-sidebar-actions">
              <button className="analyze-sidebar-reveal-btn" onClick={() => reveal(shown.path)}>
                Reveal in Finder
              </button>
              <button
                className="analyze-sidebar-trash-btn"
                onClick={() => setDeleteTarget(shown)}
                disabled={trashDisabled}
                title={isProtected(shown.path) ? "Protected location" : useTrash ? "Move to Trash" : "Delete"}
              >
                <Trash2 size={13} strokeWidth={2} />
              </button>
            </div>
          </div>
        ) : (
          <div className="analyze-sidebar-header">
            <div className="analyze-sidebar-section-label">Large files</div>
            <div className="analyze-sidebar-title">{busy ? "Looking…" : "Nothing here"}</div>
          </div>
        )}
        <div className="analyze-sidebar-list" />
        <CatTip text={tip} />
      </div>

      <DeleteConfirmDialog
        visible={deleteTarget !== null}
        title={`Delete "${deleteTarget?.name ?? ""}" (${formatSize(deleteTarget?.size ?? 0)})?`}
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
      <DeleteConfirmDialog
        visible={browserTarget !== null}
        title="This is personal data"
        description={`"${browserTarget?.name ?? ""}" holds data that can't be downloaded again, like a browser profile, wallet, message history or backup.${useTrash ? " It goes to the Trash, so you can still restore it." : " This can't be undone."}`}
        confirmLabel="Delete anyway"
        destructive
        onConfirm={handleBrowserConfirm}
        onCancel={() => setBrowserTarget(null)}
      />
    </>
  );
}

export default function Analyze() {
  const phase = useAnalyzeStore((s) => s.phase);
  const error = useAnalyzeStore((s) => s.error);
  const activeTab = useAnalyzeStore((s) => s.activeTab);
  const scanPath = useAnalyzeStore((s) => s.scanPath);
  const thresholdMb = useSettingsStore((s) => s.settings.large_file_threshold_mb);

  useEffect(() => {
    const s = useAnalyzeStore.getState();
    if (s.phase === "idle" && !s.error) s.scan();
  }, []);

  useEffect(() => {
    if (activeTab === "large-files") useAnalyzeStore.getState().ensureLargeFiles();
  }, [activeTab, scanPath, thresholdMb]);

  const largeMode = activeTab === "large-files";

  const failed = phase === "idle" && !!error;
  const scanning = phase === "scanning" || (phase === "idle" && !error);

  return (
    <div className="analyze-container">
      {error && phase === "ready" && (
        <div className="analyze-error" onClick={() => useAnalyzeStore.setState({ error: null })}>
          {error}
        </div>
      )}
      <TopBar scanning={scanning} />
      <div className="analyze-layout">
        {largeMode && <LargeFilesView />}
        {!largeMode && phase === "ready" && <ReadyView />}
        {!largeMode && scanning && (
          <>
            <ScanningPanel />
            <ScanningSidebar />
          </>
        )}
        {!largeMode && failed && <ErrorPanel />}
      </div>
    </div>
  );
}
