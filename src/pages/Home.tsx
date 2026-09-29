import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Trash2,
  Zap,
  Grid2x2Plus,
  HardDrive,
  Activity,
  Package,
  Download,
} from "lucide-react";
import ModuleCard from "../components/ModuleCard";
import { useUninstallStore } from "../stores/uninstallStore";
import { useSummaryStore } from "../stores/summaryStore";
import { useSystemStore } from "../stores/systemStore";
import { getTotalBytesFreed } from "../lib/tauri";
import { homeSpeech, tileValue, daysSinceOptimized } from "../utils/homeSummary";
import catFrame1 from "../assets/cat-tail/cat1.png";
import catFrame2 from "../assets/cat-tail/cat2.png";
import catFrame3 from "../assets/cat-tail/cat3.png";
import catFrame4 from "../assets/cat-tail/cat4.png";
import catFrame5 from "../assets/cat-tail/cat5.png";
import catFrame6 from "../assets/cat-tail/cat6.png";
import catFrame7 from "../assets/cat-tail/cat7.png";
import "../styles/dashboard.css";

export default function Home() {
  const navigate = useNavigate();
  const [totalFreed, setTotalFreed] = useState(0);
  const uninstallApps = useUninstallStore((s) => s.apps);
  const uninstallPhase = useUninstallStore((s) => s.phase);
  const scanApps = useUninstallStore((s) => s.scanApps);

  // Load lifetime stats
  useEffect(() => {
    getTotalBytesFreed().then(setTotalFreed).catch(() => {});
  }, []);

  // Animated counter
  const [displayBytes, setDisplayBytes] = useState(0);
  const animRef = useRef(0);
  const animate = useCallback((target: number) => {
    cancelAnimationFrame(animRef.current);
    const duration = 2000;
    const start = performance.now();
    const from = 0;
    const tick = (now: number) => {
      const t = Math.min((now - start) / duration, 1);
      const ease = t < 0.5
        ? 4 * t * t * t
        : 1 - (-2 * t + 2) ** 3 / 2; // ease-in-out cubic
      setDisplayBytes(Math.round(from + (target - from) * ease));
      if (t < 1) animRef.current = requestAnimationFrame(tick);
    };
    animRef.current = requestAnimationFrame(tick);
  }, []);

  useEffect(() => {
    if (totalFreed > 0) animate(totalFreed);
  }, [totalFreed, animate]);

  // Scan apps lazily — only once, not on every dashboard visit
  const hasScannedRef = useRef(false);
  useEffect(() => {
    if (uninstallPhase === "idle" && !hasScannedRef.current) {
      hasScannedRef.current = true;
      scanApps();
    }
  }, [uninstallPhase, scanApps]);

  // Cat sprite animation
  const catFrames = [catFrame1, catFrame2, catFrame3, catFrame4, catFrame5, catFrame6, catFrame7, catFrame6, catFrame5, catFrame4, catFrame3, catFrame2];
  const [catIdx, setCatIdx] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCatIdx(i => (i + 1) % 12), 220);
    return () => clearInterval(id);
  }, []);

  const summaries = useSummaryStore();
  const stats = useSystemStore((s) => s.stats);
  const fetchStats = useSystemStore((s) => s.fetchStats);
  useEffect(() => {
    fetchStats();
    const id = setInterval(fetchStats, 3000);
    return () => clearInterval(id);
  }, [fetchStats]);

  const GB = 1024 * 1024 * 1024;
  const reclaimedGB = displayBytes / GB;

  const diskUsed = stats ? stats.disk_total - stats.disk_free : 0;
  const diskPct = stats && stats.disk_total > 0 ? Math.round((diskUsed / stats.disk_total) * 100) : 0;

  const speech = homeSpeech(summaries, stats);

  const heroSub = summaries.firstCleanAt
    ? `since ${new Date(summaries.firstCleanAt).toLocaleDateString(undefined, { month: "short", year: "numeric" })} · ${summaries.cleans} ${summaries.cleans === 1 ? "clean" : "cleans"}`
    : totalFreed > 0 ? "all-time total" : "No cleans yet";

  const optimizeDays = daysSinceOptimized(summaries.optimizedAt);

  return (
    <div className="home-container">
      <div className="bento-grid">
        {/* Row 1 — Hero banner */}
        <div className="hero-banner" style={{ gridColumn: "1 / 5" }}>
          <img
            src={catFrames[catIdx]}
            alt=""
            className="hero-cat"
          />
          <div className="hero-middle">
            <div className="hero-bubble">
              {speech.a}<span style={{ color: "#1f5fff" }}>{speech.b}</span>{speech.c}
            </div>
            <div className="hero-actions">
              <button className="btn btn-primary" style={{ height: 34, padding: "0 16px", borderRadius: 999 }} onClick={() => navigate(speech.to)}>
                {speech.cta}
              </button>
            </div>
          </div>
          <div className="hero-content">
            <div className="hero-label">Reclaimed together</div>
            <div className="hero-counter">
              <span className="hero-number">
                {reclaimedGB.toFixed(1)}
              </span>
              <span className="hero-unit">GB</span>
            </div>
            <div className="hero-sub">
              {heroSub}
            </div>
          </div>
        </div>

        {/* Row 2 — Tinted tiles */}
        <ModuleCard
          title="Prune"
          icon={Package}
          route="/prune"
          size="wide"
          value={tileValue(summaries.prune?.bytes)}
          meta={summaries.prune
            ? summaries.prune.bytes > 0
              ? `Build artifacts in ${summaries.prune.projects} ${summaries.prune.projects === 1 ? "project" : "projects"}`
              : "No stale builds"
            : "node_modules, dist, target"}
          tint="red"
          flag={(summaries.prune?.bytes ?? 0) > 0}
          style={{ gridColumn: "span 2" }}
        />

        <ModuleCard
          title="Installers"
          icon={Download}
          route="/installers"
          size="big"
          value={tileValue(summaries.installers?.bytes)}
          meta={summaries.installers
            ? summaries.installers.count > 0
              ? `${summaries.installers.count} installer ${summaries.installers.count === 1 ? "file" : "files"}`
              : "No installers found"
            : "Scan downloads"}
          tint="yellow"
          flag={(summaries.installers?.bytes ?? 0) > 0}
        />

        <ModuleCard
          title="Clean"
          icon={Trash2}
          route="/clean"
          size="big"
          value={tileValue(summaries.clean?.bytes)}
          meta={summaries.clean
            ? summaries.clean.bytes > 0 ? "Caches and logs" : "Caches are tidy"
            : "System caches & logs"}
          tint="green"
          flag={(summaries.clean?.bytes ?? 0) > 0}
        />

        {/* Row 3 — Small neutral tiles */}
        <ModuleCard
          title="Analyze"
          icon={HardDrive}
          route="/analyze"
          value={stats ? `${Math.round(diskUsed / GB)} GB` : undefined}
          meta={stats ? `${diskPct}% of ${Math.round(stats.disk_total / GB)} GB used` : "Disk usage"}
        />

        <ModuleCard
          title="Status"
          icon={Activity}
          route="/status"
          value={stats ? `CPU ${Math.round(stats.cpu_usage)}%` : undefined}
          meta={stats ? `Memory ${Math.round(stats.memory_percent)}%` : "Live monitoring"}
        />

        <ModuleCard
          title="Uninstall"
          icon={Grid2x2Plus}
          route="/uninstall"
          value={uninstallApps.length > 0 ? `${uninstallApps.length} apps` : undefined}
          meta={uninstallApps.length > 0 ? "Installed apps" : "Scan apps"}
        />

        <ModuleCard
          title="Optimize"
          icon={Zap}
          route="/optimize"
          value={optimizeDays === null ? undefined : optimizeDays === 0 ? "Today" : `${optimizeDays} ${optimizeDays === 1 ? "day" : "days"}`}
          meta={optimizeDays === null ? "No tune-up yet" : optimizeDays === 0 ? "Tuned up" : "since last tune-up"}
        />
      </div>
    </div>
  );
}
