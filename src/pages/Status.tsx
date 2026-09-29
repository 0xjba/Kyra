import { useEffect, useId, memo, type ReactNode } from "react";
import { BatteryMedium, Cpu, Plug, Thermometer } from "lucide-react";
import { useStatusStore } from "../stores/statusStore";
import type { TopProcess } from "../lib/tauri";
import "../styles/status.css";

const EMPTY_PROCESSES: TopProcess[] = [];
const GIB = 1024 * 1024 * 1024;
const MIB = 1024 * 1024;
const CHART_SLOTS = 40;
const CHART_W = 300;
const CHART_H = 80;

function formatRate(bytesPerSec: number): string {
  if (bytesPerSec >= 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(1)} GB/s`;
  if (bytesPerSec >= 1000 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
  if (bytesPerSec >= 1024) return `${Math.round(bytesPerSec / 1024)} KB/s`;
  return `${Math.round(bytesPerSec)} B/s`;
}

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const mins = Math.floor((secs % 3600) / 60);
  if (days >= 1) return `${days}d ${hours}h`;
  if (hours >= 1) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function formatDiskGb(bytes: number): string {
  const gb = bytes / 1e9;
  if (gb >= 1000) return `${(gb / 1000).toFixed(1)} TB`;
  if (gb >= 100) return `${Math.round(gb)} GB`;
  return `${gb.toFixed(1)} GB`;
}

function formatProcMem(bytes: number): string {
  if (bytes >= 1000 * MIB) return `${(bytes / GIB).toFixed(1)} GB`;
  return `${Math.round(bytes / MIB)} MB`;
}

interface GaugeProps {
  percent: number;
  label: string;
  detail: string;
  ink: string;
  tone: "cpu" | "memory" | "disk";
}

const RING_R = 33;
const RING_C = 2 * Math.PI * RING_R;

const GaugeCard = memo(function GaugeCard({ percent, label, detail, ink, tone }: GaugeProps) {
  const pct = Math.max(0, Math.min(100, isFinite(percent) ? percent : 0));
  return (
    <div className={`status-gauge-card status-gauge-${tone}`}>
      <div className="status-gauge-ring">
        <svg className="status-gauge-svg" width="78" height="78" viewBox="0 0 78 78">
          <circle className="status-gauge-track" cx="39" cy="39" r={RING_R} fill="none" strokeWidth="8" />
          <circle
            cx="39"
            cy="39"
            r={RING_R}
            fill="none"
            stroke={ink}
            strokeWidth="8"
            strokeLinecap="round"
            strokeDasharray={RING_C}
            strokeDashoffset={RING_C * (1 - pct / 100)}
            className="status-gauge-fill"
          />
        </svg>
        <div className="status-gauge-percent">{Math.round(pct)}%</div>
      </div>
      <div className="status-gauge-text">
        <div className="status-gauge-label">{label}</div>
        <div className="status-gauge-detail">{detail}</div>
      </div>
    </div>
  );
});

const NetworkCard = memo(function NetworkCard() {
  const history = useStatusStore((s) => s.networkHistory);
  const gradId = useId();

  const visible = history.slice(-CHART_SLOTS);
  const latest = visible.length > 0 ? visible[visible.length - 1] : null;
  const peak = visible.reduce((m, p) => Math.max(m, p.download, p.upload), 0);
  const maxVal = Math.max(4096, peak / 0.92);
  const step = CHART_W / (CHART_SLOTS - 1);
  const offset = CHART_SLOTS - visible.length;

  const toPoints = (vals: number[]) =>
    vals
      .map((v, i) => {
        const x = (offset + i) * step;
        const y = CHART_H - (Math.max(0, v) / maxVal) * (CHART_H - 4);
        return `${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(" ");

  const canDraw = visible.length >= 2;
  const dlPts = canDraw ? toPoints(visible.map((p) => p.download)) : "";
  const ulPts = canDraw ? toPoints(visible.map((p) => p.upload)) : "";
  const startX = (offset * step).toFixed(2);
  const dlArea = canDraw ? `${startX},${CHART_H} ${dlPts} ${CHART_W},${CHART_H}` : "";

  return (
    <div className="status-network-card">
      <div className="status-network-header">
        <span className="status-network-label">Network</span>
        <span className="status-network-down">
          <span className="status-network-swatch-down" />
          {"↓"} {latest ? formatRate(latest.download) : "0 B/s"}
        </span>
        <span className="status-network-up">
          <span className="status-network-swatch-up" />
          {"↑"} {latest ? formatRate(latest.upload) : "0 B/s"}
        </span>
      </div>
      <svg
        className="status-network-graph"
        viewBox={`0 0 ${CHART_W} ${CHART_H}`}
        preserveAspectRatio="none"
      >
        <defs>
          <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#1f5fff" stopOpacity="0.28" />
            <stop offset="1" stopColor="#1f5fff" stopOpacity="0" />
          </linearGradient>
        </defs>
        {canDraw && (
          <>
            <polygon points={dlArea} fill={`url(#${gradId})`} />
            <polyline
              points={dlPts}
              fill="none"
              stroke="#1f5fff"
              strokeWidth="1.8"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
            <polyline
              points={ulPts}
              fill="none"
              stroke="#8E5CF6"
              strokeWidth="1.4"
              strokeDasharray="4 3"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          </>
        )}
      </svg>
    </div>
  );
});

interface InfoRow {
  k: string;
  v: string;
}

function InfoCard({ icon, title, rows }: { icon: ReactNode; title: string; rows: InfoRow[] }) {
  return (
    <div className="status-info-card">
      <div className="status-info-title">
        <span className="status-info-icon">{icon}</span>
        <span className="status-info-label">{title}</span>
      </div>
      {rows.map((r) => (
        <div key={r.k} className="status-info-row">
          <span className="status-info-key">{r.k}</span>
          <span className="status-info-value">{r.v}</span>
        </div>
      ))}
    </div>
  );
}

const ThermalCard = memo(function ThermalCard() {
  const cpuTemp = useStatusStore((s) => s.stats?.cpu_temp ?? -1);
  const gpuTemp = useStatusStore((s) => s.stats?.gpu_temp ?? -1);
  const ssdTemp = useStatusStore((s) => s.stats?.ssd_temp ?? -1);
  const pressure = useStatusStore((s) => s.stats?.thermal_pressure ?? "nominal");

  const rows: InfoRow[] = [];
  if (cpuTemp > 0) rows.push({ k: "CPU", v: `${Math.round(cpuTemp)}°C` });
  if (gpuTemp > 0) rows.push({ k: "GPU", v: `${Math.round(gpuTemp)}°C` });
  if (ssdTemp > 0) rows.push({ k: "SSD", v: `${Math.round(ssdTemp)}°C` });
  if (rows.length === 0) {
    rows.push({ k: "Pressure", v: pressure === "throttled" ? "Throttled" : "Nominal" });
  }

  return <InfoCard icon={<Thermometer size={14} strokeWidth={2} />} title="Thermals" rows={rows} />;
});

const GpuCard = memo(function GpuCard() {
  const gpuName = useStatusStore((s) => s.stats?.gpu_name ?? "Unknown");
  const gpuExtra = useStatusStore((s) => s.stats?.gpu_vram ?? "N/A");

  const name = gpuName === "Unknown" ? "" : gpuName.replace(/^Apple\s+/, "");
  const coreCount = /^\d+$/.test(gpuExtra.trim()) ? gpuExtra.trim() : "";
  const rows: InfoRow[] = [];
  if (name) rows.push({ k: "Chip", v: coreCount ? `${name} · ${coreCount}-core` : name });
  if (!coreCount && gpuExtra !== "N/A" && gpuExtra.trim()) rows.push({ k: "VRAM", v: gpuExtra.trim() });
  if (rows.length === 0) rows.push({ k: "Chip", v: "—" });

  return <InfoCard icon={<Cpu size={14} strokeWidth={2} />} title="GPU" rows={rows} />;
});

const BatteryCard = memo(function BatteryCard() {
  const percent = useStatusStore((s) => s.stats?.battery_percent ?? -1);
  const charging = useStatusStore((s) => s.stats?.battery_charging ?? false);
  const health = useStatusStore((s) => s.stats?.battery_health ?? "N/A");
  const cycleCount = useStatusStore((s) => s.stats?.battery_cycle_count ?? -1);

  if (percent < 0) {
    return (
      <InfoCard
        icon={<Plug size={14} strokeWidth={2} />}
        title="Power"
        rows={[{ k: "Source", v: "Power adapter" }]}
      />
    );
  }

  const rows: InfoRow[] = [
    { k: "Charge", v: `${Math.round(percent)}%${charging ? " · Charging" : ""}` },
  ];
  if (health && health !== "N/A") rows.push({ k: "Health", v: health });
  if (cycleCount >= 0) rows.push({ k: "Cycles", v: String(cycleCount) });

  return <InfoCard icon={<BatteryMedium size={14} strokeWidth={2} />} title="Battery" rows={rows} />;
});

const TopProcesses = memo(function TopProcesses() {
  const processes = useStatusStore((s) => s.stats?.top_processes ?? EMPTY_PROCESSES);
  const shown = processes.slice(0, 5);
  const barMax = Math.max(14, ...shown.map((p) => p.cpu));

  return (
    <div className="status-process-card">
      <div className="status-process-header">
        <span>Top processes</span>
        <span className="status-process-num">CPU</span>
        <span className="status-process-num">Memory</span>
      </div>
      {shown.length === 0
        ? Array.from({ length: 5 }).map((_, i) => (
            <div key={i} className="status-process-row status-skeleton-row">
              <span className="status-process-name">
                <span className="status-process-bar" />
                <span className="status-skeleton status-skeleton-name" />
              </span>
              <span className="status-process-num"><span className="status-skeleton status-skeleton-cpu" /></span>
              <span className="status-process-num"><span className="status-skeleton status-skeleton-mem" /></span>
            </div>
          ))
        : shown.map((proc, i) => (
            <div key={`${proc.name}-${i}`} className="status-process-row">
              <span className="status-process-name">
                <span className="status-process-bar">
                  <span
                    className="status-process-bar-fill"
                    style={{ width: `${Math.min(100, (Math.max(0, proc.cpu) / barMax) * 100)}%` }}
                  />
                </span>
                <span className="status-process-title">{proc.name}</span>
              </span>
              <span className="status-process-cpu">{proc.cpu.toFixed(1)}%</span>
              <span className="status-process-mem">{formatProcMem(proc.memory)}</span>
            </div>
          ))}
    </div>
  );
});

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

  if (!stats) {
    return (
      <div className="status-container">
        <div className="status-loading">
          <div className="spinner" />
          <span className="status-loading-text">Loading system info…</span>
        </div>
      </div>
    );
  }

  const chip = stats.gpu_name && stats.gpu_name !== "Unknown" ? stats.gpu_name.replace(/^Apple\s+/, "") : "";
  const machineLine = [
    stats.device_name,
    chip,
    stats.os_version ? `macOS ${stats.os_version}` : "",
  ]
    .filter(Boolean)
    .join(" · ");

  const cores = stats.cpu_cores.length;
  const cpuDetail = [
    cores > 0 ? `${cores} cores` : "",
    stats.cpu_temp > 0 ? `${Math.round(stats.cpu_temp)}°C` : "",
  ]
    .filter(Boolean)
    .join(" · ") || "—";

  const memDetail = `${(stats.memory_used / GIB).toFixed(1)} of ${Math.round(stats.memory_total / GIB)} GB`;
  const diskDetail = `${formatDiskGb(stats.disk_free)} free of ${formatDiskGb(stats.disk_total)}`;

  return (
    <div className="status-container">
      <div className="status-header">
        {machineLine && <span className="status-machine">{machineLine}</span>}
        {stats.uptime_secs > 0 && (
          <span className="status-uptime">
            <span className="status-uptime-dot" />
            Up {formatUptime(stats.uptime_secs)}
          </span>
        )}
      </div>

      <div className="status-gauges">
        <GaugeCard tone="cpu" label="CPU" percent={stats.cpu_usage} detail={cpuDetail} ink="#0b8fd0" />
        <GaugeCard tone="memory" label="Memory" percent={stats.memory_percent} detail={memDetail} ink="#1a9e40" />
        <GaugeCard tone="disk" label="Disk" percent={stats.disk_percent} detail={diskDetail} ink="#c99400" />
      </div>

      <NetworkCard />

      <div className="status-info-strip">
        <ThermalCard />
        <GpuCard />
        <BatteryCard />
      </div>

      <TopProcesses />
    </div>
  );
}
