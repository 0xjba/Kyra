import { useEffect } from "react";
import { useStatusStore } from "../stores/statusStore";
import ArcGauge from "../components/ArcGauge";
import NetworkGraph from "../components/NetworkGraph";
import "../styles/status.css";

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

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
          color="var(--cyan)"
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
          color="var(--green)"
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
          color="var(--yellow)"
        />
      </div>

      <div className="status-network">
        <NetworkGraph
          history={networkHistory}
          width={460}
          height={120}
        />
      </div>

      <CpuCores />
    </div>
  );
}
