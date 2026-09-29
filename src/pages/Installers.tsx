import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Check } from "lucide-react";
import { useInstallersStore } from "../stores/installersStore";
import { useSettingsStore } from "../stores/settingsStore";
import { formatSize } from "../utils/format";
import { downloadedAgo } from "../utils/relativeTime";
import { getSystemStats, getTotalBytesFreed, type InstallerFile } from "../lib/tauri";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import SuccessOverlay from "../components/SuccessOverlay";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import "../styles/installers.css";

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];

const EXT_COLORS: Record<string, string> = {
  dmg: "#3A7BFF",
  pkg: "#FD8C34",
  mpkg: "#FD8C34",
  iso: "#8E5CF6",
  xip: "#13D1BB",
  app: "#FD4841",
  zip: "#D99A00",
};

const GB = 1024 * 1024 * 1024;
const MB = 1024 * 1024;

function fmt(bytes: number): string {
  if (bytes >= 0.995 * GB) return `${(bytes / GB).toFixed(1)} GB`;
  if (bytes >= MB) return `${Math.round(bytes / MB)} MB`;
  if (bytes > 0) return formatSize(bytes);
  return "0 MB";
}

function locationOf(path: string): string {
  if (path.includes("/Telegram Desktop")) return "Telegram";
  if (path.includes("Mail Downloads")) return "Mail";
  if (path.includes("com~apple~CloudDocs")) return "iCloud Drive";
  const m = path.match(/^\/Users\/[^/]+\/(Downloads|Desktop|Documents)\//);
  if (m) return m[1];
  if (path.includes("/Library/Downloads")) return "Library";
  return "other folders";
}

function locationsLabel(files: InstallerFile[]): string {
  const bytes = new Map<string, number>();
  for (const f of files) {
    const loc = locationOf(f.path);
    bytes.set(loc, (bytes.get(loc) ?? 0) + f.size);
  }
  const locs = [...bytes.entries()].sort((a, b) => b[1] - a[1]).map(([l]) => l);
  if (locs.length === 0) return "left to toss";
  if (locs.length === 1) return `in ${locs[0]}`;
  if (locs.length <= 3) return `in ${locs.slice(0, -1).join(", ")} and ${locs[locs.length - 1]}`;
  return `across ${locs.length} folders`;
}

function useCatFrame() {
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setFrame((f) => (f + 1) % CAT_FRAMES.length), 220);
    return () => clearInterval(id);
  }, []);
  return CAT_FRAMES[frame];
}

function HeaderCard({
  headline,
  subline,
  error,
  total,
  totalSub,
}: {
  headline: string;
  subline: string;
  error?: string | null;
  total?: string;
  totalSub?: string;
}) {
  const catSrc = useCatFrame();
  return (
    <div className="ins-head">
      <img className="ins-head-cat" src={catSrc} alt="" />
      <div className="ins-head-text">
        <div className="ins-head-title">{headline}</div>
        <div className={`ins-head-sub${error ? " error" : ""}`}>{error || subline}</div>
      </div>
      {total !== undefined && (
        <div className="ins-head-total">
          <div className="ins-head-total-value">{total}</div>
          <div className="ins-head-total-sub">{totalSub}</div>
        </div>
      )}
    </div>
  );
}

function DocIcon({ ext }: { ext: string }) {
  const e = ext.toLowerCase();
  return (
    <div className="ins-doc">
      <span className="ins-doc-tag" style={{ background: EXT_COLORS[e] ?? "#3A7BFF" }}>
        .{e}
      </span>
    </div>
  );
}

function Footer({ info, children }: { info: ReactNode; children: ReactNode }) {
  return (
    <div className="ins-footer">
      <span className="ins-footer-info">{info}</span>
      <div className="ins-footer-actions">{children}</div>
    </div>
  );
}

function ErrorView() {
  const scan = useInstallersStore((s) => s.scan);
  const error = useInstallersStore((s) => s.error);

  return (
    <>
      <HeaderCard
        headline="I couldn't finish looking."
        subline="Something got in the way while checking your folders."
        error={error}
      />
      <div className="ins-flex-spacer" />
      <Footer info="Scan failed">
        <button className="ins-btn-primary" onClick={scan}>
          Try again
        </button>
      </Footer>
    </>
  );
}

function ScanningView() {
  return (
    <>
      <HeaderCard
        headline="Sniffing out installers…"
        subline="Checking Downloads, Desktop, Documents, Mail, iCloud and Homebrew downloads."
      />
      <div className="ins-grid">
        {Array.from({ length: 6 }, (_, i) => (
          <div key={i} className="ins-card ins-card-skeleton" style={{ animationDelay: `${i * 120}ms` }}>
            <div className="ins-card-top">
              <div className="ins-doc ins-doc-skeleton" />
              <div className="ins-card-meta">
                <div className="ins-skel-line" style={{ width: "80%" }} />
                <div className="ins-skel-line short" style={{ width: "55%" }} />
              </div>
            </div>
            <div className="ins-card-bottom">
              <div className="ins-skel-line size" />
            </div>
          </div>
        ))}
      </div>
      <Footer info="Scanning…">
        <button className="ins-btn-primary" disabled>
          Move to Trash
        </button>
      </Footer>
    </>
  );
}

function ListView() {
  const phase = useInstallersStore((s) => s.phase);
  const files = useInstallersStore((s) => s.files);
  const selected = useInstallersStore((s) => s.selected);
  const progress = useInstallersStore((s) => s.progress);
  const error = useInstallersStore((s) => s.error);
  const freed = useInstallersStore((s) => s.freed);
  const toggleSelect = useInstallersStore((s) => s.toggleSelect);
  const selectAll = useInstallersStore((s) => s.selectAll);
  const deselectAll = useInstallersStore((s) => s.deselectAll);
  const deleteSelected = useInstallersStore((s) => s.deleteSelected);
  const scan = useInstallersStore((s) => s.scan);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);
  const [showConfirm, setShowConfirm] = useState(false);

  const deleting = phase === "deleting";
  const sorted = useMemo(() => [...files].sort((a, b) => b.size - a.size), [files]);
  const totalSize = files.reduce((sum, f) => sum + f.size, 0);
  const selectedFiles = files.filter((f) => selected.has(f.path));
  const selectedSize = selectedFiles.reduce((sum, f) => sum + f.size, 0);
  const allSelected = files.length > 0 && selected.size === files.length;
  const empty = files.length === 0;

  const btnLabel = deleting
    ? `${useTrash ? "Moving" : "Deleting"}… ${
        progress && progress.items_total > 0
          ? Math.round((progress.items_done / progress.items_total) * 100)
          : 0
      }%`
    : selected.size > 0
      ? useTrash
        ? `Move ${fmt(selectedSize)} to Trash`
        : `Delete ${fmt(selectedSize)}`
      : useTrash
        ? "Move to Trash"
        : "Delete";

  const headline = empty
    ? "No installers lying around."
    : freed > 0
      ? `Tossed ${fmt(freed)}. The rest look like keepers.`
      : `Found ${files.length} installer${files.length === 1 ? "" : "s"}. Toss them?`;
  const subline = empty
    ? "Checked Downloads, Desktop, Documents, Mail, iCloud and Homebrew downloads."
    : "Installers are safe to delete once the app is in Applications.";

  const info = deleting
    ? progress?.current_item
      ? `Removing ${progress.current_item}…`
      : "Starting…"
    : `${selected.size} of ${files.length} selected`;

  return (
    <>
      <HeaderCard
        headline={headline}
        subline={subline}
        total={fmt(totalSize)}
        totalSub={locationsLabel(files)}
      />

      <div className="ins-grid">
        {sorted.map((file) => {
          const on = selected.has(file.path);
          return (
            <div
              key={file.path}
              className={`ins-card${on ? " selected" : ""}${deleting && on ? " leaving" : ""}`}
              onClick={() => !deleting && toggleSelect(file.path)}
            >
              <div className="ins-card-top">
                <DocIcon ext={file.extension} />
                <div className="ins-card-meta">
                  <div className="ins-card-name" title={file.name}>{file.name}</div>
                  <div className="ins-card-age">{downloadedAgo(file.modified_secs)}</div>
                </div>
                <span className={`ins-check${on ? " checked" : ""}`}>
                  {on && <Check size={11} strokeWidth={3} />}
                </span>
              </div>
              <div className="ins-card-bottom">
                <span className="ins-card-size">{fmt(file.size)}</span>
              </div>
            </div>
          );
        })}
      </div>

      <Footer info={error ? <span className="ins-footer-error">{error}</span> : info}>
        {empty ? (
          <button className="ins-btn-secondary" onClick={scan}>
            Scan Again
          </button>
        ) : (
          <button
            className="ins-btn-secondary"
            disabled={deleting}
            onClick={allSelected ? deselectAll : selectAll}
          >
            {allSelected ? "Deselect All" : "Select All"}
          </button>
        )}
        <button
          className="ins-btn-primary"
          disabled={deleting || selected.size === 0}
          onClick={() => setShowConfirm(true)}
        >
          {btnLabel}
        </button>
      </Footer>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`${useTrash ? "Move" : "Delete"} ${selected.size} file${selected.size === 1 ? "" : "s"} (${formatSize(selectedSize)})${useTrash ? " to Trash" : ""}?`}
        onConfirm={() => {
          setShowConfirm(false);
          deleteSelected();
        }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

function DoneView() {
  const result = useInstallersStore((s) => s.result);
  const files = useInstallersStore((s) => s.files);
  const dismissDone = useInstallersStore((s) => s.dismissDone);
  const useTrash = useSettingsStore((s) => s.settings.use_trash);
  const [diskTotal, setDiskTotal] = useState(0);
  const [diskFree, setDiskFree] = useState(0);
  const [lifetimeBytes, setLifetimeBytes] = useState(0);

  useEffect(() => {
    getSystemStats()
      .then((s) => {
        setDiskTotal(s.disk_total);
        setDiskFree(s.disk_free);
      })
      .catch(() => {});
    getTotalBytesFreed().then(setLifetimeBytes).catch(() => {});
  }, []);

  const deleted = new Set(result?.deleted_paths ?? []);
  const categoryCount = new Set(
    files.filter((f) => deleted.has(f.path)).map((f) => f.extension.toLowerCase()),
  ).size;

  return (
    <SuccessOverlay
      headline="Tossed"
      freedGB={(result?.bytes_freed ?? 0) / GB}
      detail={useTrash ? "of installers moved to the Trash" : "of installers deleted"}
      itemCount={result?.items_removed ?? 0}
      categoryCount={categoryCount}
      lifetimeGB={lifetimeBytes / GB}
      storageUsedGB={(diskTotal - diskFree) / GB}
      storageTotalGB={diskTotal / GB}
      showPawtrolUpsell={true}
      onDone={dismissDone}
    />
  );
}

export default function Installers() {
  const phase = useInstallersStore((s) => s.phase);
  const error = useInstallersStore((s) => s.error);
  const scan = useInstallersStore((s) => s.scan);

  useEffect(() => {
    if (useInstallersStore.getState().phase === "idle" && !useInstallersStore.getState().error) scan();
  }, [scan]);

  return (
    <div className="ins-root">
      {phase === "idle" && (error ? <ErrorView /> : <ScanningView />)}
      {phase === "scanning" && <ScanningView />}
      {(phase === "list" || phase === "deleting") && <ListView />}
      {phase === "done" && <DoneView />}
    </div>
  );
}
