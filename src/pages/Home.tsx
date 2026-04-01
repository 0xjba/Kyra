import { useEffect, useRef, useState } from "react";
import {
  Trash2,
  Zap,
  Grid2x2Plus,
  PieChart,
  Activity,
  Package,
  Disc,
  Settings as SettingsIcon,
} from "lucide-react";
import ModuleCard from "../components/ModuleCard";
import { useUninstallStore } from "../stores/uninstallStore";
import { getTotalBytesFreed } from "../lib/tauri";
import { formatSize } from "../utils/format";
import "../styles/dashboard.css";

export default function Home() {
  const [totalFreed, setTotalFreed] = useState(0);
  const uninstallApps = useUninstallStore((s) => s.apps);
  const uninstallPhase = useUninstallStore((s) => s.phase);
  const scanApps = useUninstallStore((s) => s.scanApps);

  // Load lifetime stats
  useEffect(() => {
    getTotalBytesFreed().then(setTotalFreed).catch(() => {});
  }, []);

  // Scan apps lazily — only once, not on every dashboard visit
  const hasScannedRef = useRef(false);
  useEffect(() => {
    if (uninstallPhase === "idle" && !hasScannedRef.current) {
      hasScannedRef.current = true;
      scanApps();
    }
  }, [uninstallPhase, scanApps]);

  /* 4-col × 3-row bento (12 cells total)
   * ┌──────────┬──────┬──────┐
   * │  Clean   │Optim.│Uninst│
   * │  (2×2)   ├──────┼──────┤
   * │          │Status│Settn.│
   * ├────┬─────┼──────┴──────┤
   * │Purg│Inst.│  Analyze    │
   * └────┴─────┴─────────────┘
   */

  return (
    <div className="home-container">
      <div className="bento-grid">
        <ModuleCard
          title="Clean"
          description="System caches, logs, and temporary files"
          icon={Trash2}
          route="/clean"
          stat={totalFreed > 0 ? formatSize(totalFreed) : "—"}
          statLabel="All time space reclaimed"
          style={{ gridColumn: "span 2", gridRow: "span 2" }}
        />

        <ModuleCard
          title="Optimize"
          description="Refresh caches, repair configs and tune performance"
          icon={Zap}
          route="/optimize"
          meta="14 tasks available"
        />

        <ModuleCard
          title="Uninstall"
          description="Remove apps with all associated files"
          icon={Grid2x2Plus}
          route="/uninstall"
          meta={uninstallApps.length > 0 ? `${uninstallApps.length} apps installed` : "Scan apps"}
        />

        <ModuleCard
          title="Status"
          description=""
          icon={Activity}
          route="/status"
          meta="Live monitoring"
        />

        <ModuleCard
          title="Settings"
          description="Preferences"
          icon={SettingsIcon}
          route="/settings"
          meta="Configure"
        />

        <ModuleCard
          title="Purge"
          description="Project build artifacts"
          icon={Package}
          route="/purge"
          meta="node_modules, dist, target"
        />

        <ModuleCard
          title="Installers"
          description="Find .dmg, .pkg, .iso"
          icon={Disc}
          route="/installers"
          meta="Scan downloads"
        />

        <ModuleCard
          title="Analyze"
          description="Explore disk usage"
          icon={PieChart}
          route="/analyze"
          meta="Scan to explore"
          style={{ gridColumn: "span 2" }}
        />
      </div>
    </div>
  );
}
