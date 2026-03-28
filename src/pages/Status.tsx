import { useEffect } from "react";
import { useStatusStore } from "../stores/statusStore";
import ArcGauge from "../components/ArcGauge";
import NetworkGraph from "../components/NetworkGraph";
import { formatSize } from "../utils/format";
import "../styles/status.css";

function CpuCores() {
  const cores = useStatusStore((s) => s.stats?.cpu_cores ?? []);

  return (
    <div className="status-cores">
      <div className="status-section-label">CPU Cores</div>
      <div className="status-core-grid">
        {cores.map((usage, i) => (
          <div key={i} className="status-core">
            <div className="status-core-bar-track">
              <div
                className="status-core-bar-fill"
                style={{ height: `${usage}%` }}
              />
            </div>
            <span className="status-core-label">{i}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export default function Status() {
  const stats = useStatusStore((s) => s.stats);
  const networkHistory = useStatusStore((s) => s.networkHistory);
  const startStream = useStatusStore((s) => s.startStream);
  const stopStream = useStatusStore((s) => s.stopStream);

  useEffect(() => {
    startStream();
    return () => stopStream();
  }, [startStream, stopStream]);

  return (
    <div className="status-container">
      <div className="status-gauges">
        <ArcGauge
          value={stats?.cpu_usage ?? 0}
          max={100}
          label="CPU"
          detail={stats ? `${stats.cpu_cores.length} cores` : "—"}
          color="#5ef5e2"
        />
        <ArcGauge
          value={stats?.memory_used ?? 0}
          max={stats?.memory_total ?? 1}
          label="Memory"
          detail={
            stats
              ? `${formatSize(stats.memory_used)} / ${formatSize(stats.memory_total)}`
              : "—"
          }
          color="#4ade80"
        />
        <ArcGauge
          value={stats?.disk_used ?? 0}
          max={stats?.disk_total ?? 1}
          label="Disk"
          detail={
            stats
              ? `${formatSize(stats.disk_free)} free`
              : "—"
          }
          color="#facc15"
        />
      </div>

      <div className="status-network">
        <NetworkGraph
          history={networkHistory}
          height={120}
        />
      </div>

      <CpuCores />
    </div>
  );
}
