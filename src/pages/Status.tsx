import { useEffect, memo } from "react";
import { useStatusStore } from "../stores/statusStore";
import ArcGauge from "../components/ArcGauge";
import NetworkGraph from "../components/NetworkGraph";
import ErrorBoundary from "../components/ErrorBoundary";
import { formatSize } from "../utils/format";
import type { TopProcess } from "../lib/tauri";
import "../styles/status.css";

const EMPTY_CORES: number[] = [];
const EMPTY_INTERFACES: any[] = [];
const EMPTY_PROCESSES: TopProcess[] = [];

const CpuCores = memo(function CpuCores() {
  const cores = useStatusStore((s) => {
    const raw = s.stats?.cpu_cores;
    return Array.isArray(raw) ? raw : EMPTY_CORES;
  });

  return (
    <div className="status-cores">
      <div className="status-section-label">CPU Cores</div>
      <div className="status-core-grid">
        {cores.map((usage, i) => {
          const pct = isFinite(usage) ? Math.min(Math.max(usage, 0), 100) : 0;
          return (
            <div key={i} className="status-core">
              <div className="status-core-bar-track">
                <div
                  className="status-core-bar-fill"
                  style={{ height: `${pct}%` }}
                />
              </div>
              <span className="status-core-label">{i}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
});

const MemoryPressure = memo(function MemoryPressure() {
  const pressure = useStatusStore((s) => s.stats?.memory_pressure ?? "normal");
  const swapTotal = useStatusStore((s) => s.stats?.swap_total ?? 0);
  const swapUsed = useStatusStore((s) => s.stats?.swap_used ?? 0);

  const dotColor =
    pressure === "critical"
      ? "var(--red)"
      : pressure === "warning"
        ? "var(--yellow)"
        : "var(--green)";

  return (
    <div className="status-info-card">
      <div className="status-section-label">Memory Pressure</div>
      <div className="status-info-row">
        <span
          className="status-dot"
          style={{ backgroundColor: dotColor }}
        />
        <span className="status-info-value">
          {pressure.charAt(0).toUpperCase() + pressure.slice(1)}
        </span>
      </div>
      {swapTotal > 0 && (
        <div className="status-info-row status-info-sub">
          <span className="status-info-label">Swap</span>
          <span className="status-info-value">
            {formatSize(swapUsed)} / {formatSize(swapTotal)}
          </span>
        </div>
      )}
    </div>
  );
});

const BatteryInfo = memo(function BatteryInfo() {
  const percent = useStatusStore((s) => s.stats?.battery_percent ?? -1);
  const charging = useStatusStore((s) => s.stats?.battery_charging ?? false);
  const timeRemaining = useStatusStore(
    (s) => s.stats?.battery_time_remaining ?? "N/A"
  );
  const health = useStatusStore((s) => s.stats?.battery_health ?? "N/A");
  const cycleCount = useStatusStore((s) => s.stats?.battery_cycle_count ?? -1);

  // No battery (desktop Mac)
  if (percent < 0) return null;

  const barColor =
    percent <= 10
      ? "var(--red)"
      : percent <= 20
        ? "var(--yellow)"
        : "var(--green)";

  return (
    <div className="status-info-card">
      <div className="status-section-label">Battery</div>
      <div className="status-battery-bar-wrap">
        <div className="status-battery-bar-track">
          <div
            className="status-battery-bar-fill"
            style={{
              width: `${Math.min(Math.max(percent, 0), 100)}%`,
              backgroundColor: barColor,
            }}
          />
        </div>
        <span className="status-info-value">
          {Math.round(percent)}%{charging ? " Charging" : ""}
        </span>
      </div>
      <div className="status-battery-details">
        <div className="status-info-row status-info-sub">
          <span className="status-info-label">Time</span>
          <span className="status-info-value">{timeRemaining}</span>
        </div>
        <div className="status-info-row status-info-sub">
          <span className="status-info-label">Health</span>
          <span className="status-info-value">{health}</span>
        </div>
        {cycleCount >= 0 && (
          <div className="status-info-row status-info-sub">
            <span className="status-info-label">Cycles</span>
            <span className="status-info-value">{cycleCount}</span>
          </div>
        )}
      </div>
    </div>
  );
});

const NetworkInterfaces = memo(function NetworkInterfaces() {
  const interfaces = useStatusStore(
    (s) => s.stats?.network_interfaces ?? EMPTY_INTERFACES
  );

  if (interfaces.length === 0) return null;

  return (
    <div className="status-info-card">
      <div className="status-section-label">Active Interfaces</div>
      {interfaces.map((iface) => (
        <div key={iface.name} className="status-iface-row">
          <span className="status-iface-name">{iface.name}</span>
          <span className="status-iface-stats">
            <span className="status-iface-up">
              {formatSize(iface.upload)}/s
            </span>
            <span className="status-iface-down">
              {formatSize(iface.download)}/s
            </span>
          </span>
        </div>
      ))}
    </div>
  );
});

const GpuInfo = memo(function GpuInfo() {
  const gpuName = useStatusStore((s) => s.stats?.gpu_name ?? "Unknown");
  const gpuVram = useStatusStore((s) => s.stats?.gpu_vram ?? "N/A");

  if (gpuName === "Unknown") return null;

  return (
    <div className="status-info-card">
      <div className="status-section-label">GPU</div>
      <div className="status-info-row">
        <span className="status-info-value">{gpuName}</span>
      </div>
      {gpuVram !== "N/A" && (
        <div className="status-info-row status-info-sub">
          <span className="status-info-label">VRAM</span>
          <span className="status-info-value">{gpuVram}</span>
        </div>
      )}
    </div>
  );
});

const ThermalInfo = memo(function ThermalInfo() {
  const pressure = useStatusStore((s) => s.stats?.thermal_pressure ?? "nominal");

  const dotColor =
    pressure === "throttled"
      ? "var(--red)"
      : "var(--green)";

  return (
    <div className="status-info-card">
      <div className="status-section-label">Thermal</div>
      <div className="status-info-row">
        <span
          className="status-dot"
          style={{ backgroundColor: dotColor }}
        />
        <span className="status-info-value">
          {pressure.charAt(0).toUpperCase() + pressure.slice(1)}
        </span>
      </div>
    </div>
  );
});

const TopProcesses = memo(function TopProcesses() {
  const processes: TopProcess[] = useStatusStore(
    (s) => s.stats?.top_processes ?? EMPTY_PROCESSES
  );

  if (processes.length === 0) return null;

  return (
    <div className="status-processes">
      <div className="status-section-label">Top Processes</div>
      <div className="status-process-list">
        {processes.map((proc, i) => (
          <div key={`${proc.name}-${i}`} className="status-process-row">
            <span className="status-process-name">{proc.name}</span>
            <span className="status-process-stats">
              <span className="status-process-cpu">
                {proc.cpu.toFixed(1)}%
              </span>
              <span className="status-process-mem">
                {formatSize(proc.memory)}
              </span>
            </span>
          </div>
        ))}
      </div>
    </div>
  );
});

const UptimeDisplay = memo(function UptimeDisplay() {
  const uptimeSecs = useStatusStore((s) => s.stats?.uptime_secs ?? 0);

  if (uptimeSecs === 0) return null;

  const days = Math.floor(uptimeSecs / 86400);
  const hours = Math.floor((uptimeSecs % 86400) / 3600);
  const mins = Math.floor((uptimeSecs % 3600) / 60);

  let display = "";
  if (days > 0) display += `${days}d `;
  if (hours > 0 || days > 0) display += `${hours}h `;
  display += `${mins}m`;

  return (
    <div className="status-info-card">
      <div className="status-section-label">Uptime</div>
      <div className="status-info-row">
        <span className="status-info-value">{display.trim()}</span>
      </div>
    </div>
  );
});

export default function Status() {
  const stats = useStatusStore((s) => s.stats);
  const networkHistory = useStatusStore((s) => s.networkHistory);
  const startStream = useStatusStore((s) => s.startStream);
  const stopStream = useStatusStore((s) => s.stopStream);

  useEffect(() => {
    startStream().catch((err) => {
      console.error("[Status] Failed to start stats stream:", err);
    });
    return () => stopStream();
  }, [startStream, stopStream]);

  return (
    <div className="status-container">
      <ErrorBoundary name="Gauges">
        <div className="status-gauges">
          <ArcGauge
            value={stats?.cpu_usage ?? 0}
            max={100}
            label="CPU"
            detail={stats ? `${stats.cpu_cores.length} cores` : "\u2014"}
            color="#22B8F0"
          />
          <ArcGauge
            value={stats?.memory_used ?? 0}
            max={stats?.memory_total ?? 1}
            label="Memory"
            detail={
              stats
                ? `${formatSize(stats.memory_used)} / ${formatSize(stats.memory_total)}`
                : "\u2014"
            }
            color="#2AC852"
          />
          <ArcGauge
            value={stats?.disk_used ?? 0}
            max={stats?.disk_total ?? 1}
            label="Disk"
            detail={
              stats
                ? `${formatSize(stats.disk_free)} free`
                : "\u2014"
            }
            color="#FDD225"
          />
        </div>
      </ErrorBoundary>

      <ErrorBoundary name="Network Graph">
        <div className="status-network">
          <NetworkGraph
            history={networkHistory}
            height={120}
          />
        </div>
      </ErrorBoundary>

      <ErrorBoundary name="Extra Metrics">
        <div className="status-extras">
          <MemoryPressure />
          <ThermalInfo />
          <UptimeDisplay />
        </div>
      </ErrorBoundary>

      <ErrorBoundary name="GPU & Battery">
        <div className="status-extras">
          <GpuInfo />
          <BatteryInfo />
          <NetworkInterfaces />
        </div>
      </ErrorBoundary>

      <ErrorBoundary name="Top Processes">
        <TopProcesses />
      </ErrorBoundary>

      <ErrorBoundary name="CPU Cores">
        <CpuCores />
      </ErrorBoundary>
    </div>
  );
}
