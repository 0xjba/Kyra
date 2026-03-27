import { useEffect } from "react";
import {
  Trash2,
  Zap,
  Grid2x2Plus,
  PieChart,
  Activity,
  Package,
  Disc,
} from "lucide-react";
import SystemStrip from "../components/SystemStrip";
import ModuleCard from "../components/ModuleCard";
import { useSystemStore } from "../stores/systemStore";
import { useCleanStore } from "../stores/cleanStore";
import "../styles/dashboard.css";

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  return `${(bytes / 1024).toFixed(0)} KB`;
}

const BAR_HEIGHTS = [45, 60, 35, 80, 55, 70, 40, 65];

export default function Home() {
  const fetchStats = useSystemStore((s) => s.fetchStats);
  const cleanItems = useCleanStore((s) => s.items);
  const reclaimable = cleanItems.reduce((sum, item) => sum + item.total_size, 0);

  useEffect(() => {
    fetchStats();
    const interval = setInterval(fetchStats, 5000);
    return () => clearInterval(interval);
  }, [fetchStats]);

  return (
    <div style={{ padding: 18, overflowY: "auto", height: "100%" }}>
      <SystemStrip />

      <div
        style={{
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-tertiary)",
          textTransform: "uppercase",
          letterSpacing: 0.5,
          margin: "18px 0 10px",
        }}
      >
        Modules
      </div>

      <div className="bento-grid">
        <ModuleCard
          title="Clean"
          description="Remove caches, logs, browser data and dev artifacts"
          icon={Trash2}
          route="/clean"
          stat={reclaimable > 0 ? formatSize(reclaimable) : "—"}
          statLabel="Reclaimable space"
          style={{ gridColumn: "span 2", gridRow: "span 2" }}
        />

        <ModuleCard
          title="Optimize"
          description="Refresh caches, repair configs and tune performance"
          icon={Zap}
          route="/optimize"
          meta="14 tasks available"
          style={{ gridColumn: "span 2", gridRow: "span 1" }}
        />

        <ModuleCard
          title="Uninstall"
          description="Remove apps completely with all associated files"
          icon={Grid2x2Plus}
          route="/uninstall"
          meta="127 apps installed"
          style={{ gridColumn: "span 2", gridRow: "span 1" }}
        />

        <ModuleCard
          title="Status"
          description=""
          icon={Activity}
          route="/status"
          style={{ gridColumn: "span 1", gridRow: "span 2" }}
        >
          <div className="mini-chart">
            {BAR_HEIGHTS.map((h, i) => (
              <div key={i} className="mini-bar" style={{ height: `${h}%` }} />
            ))}
          </div>
          <div
            style={{
              fontSize: 11,
              color: "var(--text-tertiary)",
              marginTop: 8,
            }}
          >
            Live monitoring
          </div>
        </ModuleCard>

        <ModuleCard
          title="Analyze"
          description=""
          icon={PieChart}
          route="/analyze"
          style={{ gridColumn: "span 2", gridRow: "span 2" }}
        >
          <div className="mini-sunburst" />
          <div
            style={{
              fontSize: 11,
              color: "var(--text-tertiary)",
              marginTop: 8,
              textAlign: "center",
            }}
          >
            Scan to explore disk usage
          </div>
        </ModuleCard>

        <ModuleCard
          title="Purge"
          description="Find and remove build artifacts — node_modules, target, dist"
          icon={Package}
          route="/purge"
          meta="Scan projects"
          style={{ gridColumn: "span 1", gridRow: "span 1" }}
        />

        <ModuleCard
          title="Installers"
          description="Find and remove installer files — .dmg, .pkg, .iso"
          icon={Disc}
          route="/installers"
          meta="Find .dmg, .pkg"
          style={{ gridColumn: "span 1", gridRow: "span 1" }}
        />
      </div>
    </div>
  );
}
