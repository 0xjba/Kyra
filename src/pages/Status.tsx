import { useEffect, useRef, useState, memo } from "react";
import { Plug } from "lucide-react";
import { useStatusStore } from "../stores/statusStore";
import { formatSize } from "../utils/format";
import type { TopProcess } from "../lib/tauri";
import "../styles/status.css";

/* ── Helpers ── */

const EMPTY_CORES: number[] = [];
const EMPTY_PROCESSES: TopProcess[] = [];

function formatRate(bytesPerSec: number): string {
  if (bytesPerSec >= 1073741824) return `${(bytesPerSec / 1073741824).toFixed(1)} GB/s`;
  if (bytesPerSec >= 1048576) return `${(bytesPerSec / 1048576).toFixed(1)} MB/s`;
  if (bytesPerSec >= 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${bytesPerSec} B/s`;
}

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const mins = Math.floor((secs % 3600) / 60);
  if (days >= 1) return `${days}d ${hours}h`;
  if (hours >= 1) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function coreColumns(count: number): number {
  if (count <= 16) return 8;
  if (count <= 64) return 16;
  return 32;
}

/* ══════════════════════════════════════════════════════════
   Gauge Ring — SVG-based, replaces canvas ArcGauge
   ══════════════════════════════════════════════════════════ */

interface GaugeProps {
  percent: number;
  label: string;
  detail: string;
  color: string;
}

const GaugeRing = memo(function GaugeRing({ percent, label, detail, color }: GaugeProps) {
  const size = 100;
  const strokeWidth = 6;
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference - (percent / 100) * circumference;
  const strokeOpacity = 0.15 + (percent / 100) * 0.35;

  return (
    <div className="status-gauge-card">
      <div className="status-gauge-ring">
        <svg
          className="status-gauge-svg"
          width={size}
          height={size}
          viewBox={`0 0 ${size} ${size}`}
        >
          <circle
            cx={size / 2} cy={size / 2} r={radius}
            fill="none"
            stroke="rgba(255, 255, 255, 0.06)"
            strokeWidth={strokeWidth}
          />
          <circle
            cx={size / 2} cy={size / 2} r={radius}
            fill="none"
            stroke={color}
            strokeOpacity={strokeOpacity}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            className="status-gauge-fill"
          />
        </svg>
        <div className="status-gauge-center">
          <span className="status-gauge-percent">{Math.round(percent)}%</span>
          <span className="status-gauge-label">{label}</span>
        </div>
      </div>
      <span className="status-gauge-detail">{detail}</span>
    </div>
  );
});

/* ══════════════════════════════════════════════════════════
   Network Graph — inline SVG polyline
   ══════════════════════════════════════════════════════════ */

const NetworkCard = memo(function NetworkCard() {
  const history = useStatusStore((s) => s.networkHistory);
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(400);
  const graphHeight = 100;

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let rafId: number;
    const observer = new ResizeObserver((entries) => {
      cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        for (const entry of entries) {
          setWidth(entry.contentRect.width - 24);
        }
      });
    });
    observer.observe(el);
    return () => { cancelAnimationFrame(rafId); observer.disconnect(); };
  }, []);

  const latest = history.length > 0 ? history[history.length - 1] : null;

  const maxVal = Math.max(
    1024,
    ...history.map((p) => Math.max(
      isFinite(p.download) ? p.download : 0,
      isFinite(p.upload) ? p.upload : 0
    ))
  );

  function toPoints(data: number[]): string {
    if (data.length < 2) return "";
    return data
      .map((val, i) => {
        const x = (i / (data.length - 1)) * width;
        const y = graphHeight - ((isFinite(val) ? val : 0) / maxVal) * (graphHeight - 4);
        return `${x},${y}`;
      })
      .join(" ");
  }

  const dlPoints = toPoints(history.map((p) => p.download));
  const ulPoints = toPoints(history.map((p) => p.upload));

  return (
    <div className="status-network-card" ref={containerRef}>
      <div className="status-network-header">
        <span className="status-network-label">Network</span>
        <div className="status-network-rates">
          <span className="status-network-down">
            {"\u2193"} {latest ? formatRate(latest.download) : "\u2014"}
          </span>
          <span className="status-network-up">
            {"\u2191"} {latest ? formatRate(latest.upload) : "\u2014"}
          </span>
        </div>
      </div>
      <svg
        className="status-network-graph"
        width={width}
        height={graphHeight}
        viewBox={`0 0 ${width} ${graphHeight}`}
      >
        {dlPoints && (
          <polyline
            points={dlPoints}
            fill="none"
            stroke="var(--cyan)"
            strokeWidth="1.5"
            strokeLinejoin="round"
          />
        )}
        {ulPoints && (
          <polyline
            points={ulPoints}
            fill="none"
            stroke="var(--text-tertiary)"
            strokeWidth="1"
            strokeDasharray="4 3"
            strokeLinejoin="round"
          />
        )}
      </svg>
    </div>
  );
});

/* ══════════════════════════════════════════════════════════
   Info Strip — Thermals / GPU / Battery
   ══════════════════════════════════════════════════════════ */

const ThermalCard = memo(function ThermalCard() {
  const pressure = useStatusStore((s) => s.stats?.thermal_pressure ?? "nominal");
  const isThrottled = pressure === "throttled";

  return (
    <div className="status-info-card">
      <span className="status-info-label">Thermals</span>
      <div className="status-info-dot-row">
        <span
          className="status-info-dot"
          style={{ backgroundColor: isThrottled ? "var(--red)" : "var(--green)" }}
        />
        <span className="status-info-value">
          {isThrottled ? "Throttled" : "Nominal"}
        </span>
      </div>
      <div className="status-info-row">
        <span className="status-info-key">Memory pressure</span>
        <MemoryPressureValue />
      </div>
      <SwapRow />
    </div>
  );
});

const MemoryPressureValue = memo(function MemoryPressureValue() {
  const pressure = useStatusStore((s) => s.stats?.memory_pressure ?? "normal");
  const color =
    pressure === "critical" ? "var(--red)"
    : pressure === "warning" ? "var(--yellow)"
    : "var(--green)";

  return (
    <div className="status-info-dot-row">
      <span className="status-info-dot" style={{ backgroundColor: color }} />
      <span className="status-info-value">
        {pressure.charAt(0).toUpperCase() + pressure.slice(1)}
      </span>
    </div>
  );
});

const SwapRow = memo(function SwapRow() {
  const swapTotal = useStatusStore((s) => s.stats?.swap_total ?? 0);
  const swapUsed = useStatusStore((s) => s.stats?.swap_used ?? 0);

  if (swapTotal === 0) return null;

  return (
    <div className="status-info-row">
      <span className="status-info-key">Swap</span>
      <span className="status-info-value">{formatSize(swapUsed)} / {formatSize(swapTotal)}</span>
    </div>
  );
});

const GpuCard = memo(function GpuCard() {
  const gpuName = useStatusStore((s) => s.stats?.gpu_name ?? "Unknown");
  const gpuVram = useStatusStore((s) => s.stats?.gpu_vram ?? "N/A");

  return (
    <div className="status-info-card">
      <span className="status-info-label">GPU</span>
      <span className="status-info-value-lg">{gpuName === "Unknown" ? "\u2014" : gpuName}</span>
      {gpuVram !== "N/A" && (
        <div className="status-info-row">
          <span className="status-info-key">VRAM</span>
          <span className="status-info-value">{gpuVram}</span>
        </div>
      )}
    </div>
  );
});

const BatteryCard = memo(function BatteryCard() {
  const percent = useStatusStore((s) => s.stats?.battery_percent ?? -1);
  const charging = useStatusStore((s) => s.stats?.battery_charging ?? false);
  const health = useStatusStore((s) => s.stats?.battery_health ?? "N/A");
  const cycleCount = useStatusStore((s) => s.stats?.battery_cycle_count ?? -1);

  if (percent < 0) {
    return (
      <div className="status-info-card">
        <span className="status-info-label">Battery</span>
        <div className="status-plugged-in">
          <Plug size={20} strokeWidth={1.5} />
          <span>Plugged in</span>
        </div>
      </div>
    );
  }

  return (
    <div className="status-info-card">
      <span className="status-info-label">Battery</span>
      <div className="status-info-row">
        <span className="status-info-key">Health</span>
        <span className="status-info-value">{health}</span>
      </div>
      {cycleCount >= 0 && (
        <div className="status-info-row">
          <span className="status-info-key">Cycles</span>
          <span className="status-info-value">{cycleCount}</span>
        </div>
      )}
      <div className="status-info-row">
        <span className="status-info-key">Charge</span>
        <span className="status-info-value">
          {Math.round(percent)}%{charging ? " (Charging)" : ""}
        </span>
      </div>
    </div>
  );
});

/* ══════════════════════════════════════════════════════════
   Top Processes
   ══════════════════════════════════════════════════════════ */

const TopProcesses = memo(function TopProcesses() {
  const processes = useStatusStore(
    (s) => s.stats?.top_processes ?? EMPTY_PROCESSES
  );

  if (processes.length === 0) return null;

  return (
    <div className="status-section">
      <div className="status-section-header">
        <span className="status-section-title">Top Processes</span>
      </div>
      {processes.slice(0, 5).map((proc, i) => (
        <div key={`${proc.name}-${i}`} className="status-process-row">
          <span className="status-process-name">{proc.name}</span>
          <span className="status-process-cpu">{proc.cpu.toFixed(1)}%</span>
          <span className="status-process-mem">{formatSize(proc.memory)}</span>
        </div>
      ))}
    </div>
  );
});

/* ══════════════════════════════════════════════════════════
   CPU Cores
   ══════════════════════════════════════════════════════════ */

const CpuCores = memo(function CpuCores() {
  const cores = useStatusStore((s) => {
    const raw = s.stats?.cpu_cores;
    return Array.isArray(raw) ? raw : EMPTY_CORES;
  });

  if (cores.length === 0) return null;

  const cols = coreColumns(cores.length);

  return (
    <div className="status-section">
      <div className="status-section-header">
        <span className="status-section-title">CPU Cores</span>
        <span className="status-section-meta">{cores.length} cores</span>
      </div>
      <div className="status-core-grid" data-cols={cols}>
        {cores.map((usage, i) => {
          const pct = isFinite(usage) ? Math.min(Math.max(usage, 0), 100) : 0;
          const fillOpacity = 0.08 + (pct / 100) * 0.25;
          return (
            <div key={i} className="status-core" title={`Core ${i}: ${Math.round(pct)}%`}>
              <div
                className="status-core-fill"
                style={{
                  height: `${pct}%`,
                  background: `rgba(255, 255, 255, ${fillOpacity})`,
                }}
              />
              <span className="status-core-num">{i}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
});

/* ══════════════════════════════════════════════════════════
   Main Component
   ══════════════════════════════════════════════════════════ */

export default function Status() {
  const stats = useStatusStore((s) => s.stats);
  const startStream = useStatusStore((s) => s.startStream);
  const stopStream = useStatusStore((s) => s.stopStream);

  useEffect(() => {
    startStream().catch((err) => {
      console.error("[Status] Failed to start stats stream:", err);
    });
    return () => stopStream();
  }, [startStream, stopStream]);

  const cpuPercent = stats?.cpu_usage ?? 0;
  const memPercent = stats?.memory_percent ?? 0;
  const diskPercent = stats?.disk_percent ?? 0;
  const uptimeSecs = stats?.uptime_secs ?? 0;

  const gpuName = stats?.gpu_name ?? "";
  const machineInfo = gpuName && gpuName !== "Unknown"
    ? gpuName.replace("Apple ", "")
    : "";

  return (
    <div className="status-container">
      {/* Header */}
      <div className="status-header">
        <div className="status-header-left">
          <span className="status-title">Status</span>
          {machineInfo && (
            <span className="status-machine">{machineInfo}</span>
          )}
        </div>
        {uptimeSecs > 0 && (
          <div className="status-uptime">
            <span className="status-uptime-dot" />
            Uptime {formatUptime(uptimeSecs)}
          </div>
        )}
      </div>

      {/* Three Gauge Rings */}
      <div className="status-gauges">
        <GaugeRing
          percent={cpuPercent}
          label="CPU"
          detail={stats ? `${stats.cpu_cores.length} cores` : "\u2014"}
          color="#22B8F0"
        />
        <GaugeRing
          percent={memPercent}
          label="Memory"
          detail={
            stats
              ? `${formatSize(stats.memory_used)} / ${formatSize(stats.memory_total)}`
              : "\u2014"
          }
          color="#2AC852"
        />
        <GaugeRing
          percent={diskPercent}
          label="Disk"
          detail={stats ? `${formatSize(stats.disk_free)} free` : "\u2014"}
          color="#FDD225"
        />
      </div>

      {/* Network */}
      <NetworkCard />

      {/* Info Strip */}
      <div className="status-info-strip">
        <ThermalCard />
        <GpuCard />
        <BatteryCard />
      </div>

      {/* Top Processes */}
      <TopProcesses />

      {/* CPU Cores */}
      <CpuCores />
    </div>
  );
}
