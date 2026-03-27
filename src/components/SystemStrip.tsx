import { useSystemStore } from "../stores/systemStore";
import { useCleanStore } from "../stores/cleanStore";

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  const gb = bytes / (1024 * 1024 * 1024);
  return `${Math.round(gb)} GB`;
}

interface StatProps {
  color: string;
  label: string;
  value: string;
}

function Stat({ color, label, value }: StatProps) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
      <div
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: `var(--${color})`,
          flexShrink: 0,
        }}
      />
      <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
        {label}
      </span>
      <span
        style={{
          fontSize: 13,
          fontWeight: 500,
          color: "var(--text-primary)",
        }}
      >
        {value}
      </span>
    </div>
  );
}

export default function SystemStrip() {
  const stats = useSystemStore((s) => s.stats);
  const cleanItems = useCleanStore((s) => s.items);

  const reclaimable = cleanItems.reduce((sum, item) => sum + item.total_size, 0);

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 24,
        padding: "12px 16px",
        background: "var(--bg-card)",
        border: "1px solid var(--border)",
        borderRadius: 10,
      }}
    >
      <Stat
        color="cyan"
        label="CPU"
        value={stats ? `${Math.round(stats.cpu_usage)}%` : "—"}
      />
      <Stat
        color="green"
        label="Memory"
        value={stats ? `${Math.round(stats.memory_percent)}%` : "—"}
      />
      <Stat
        color="yellow"
        label="Disk"
        value={stats ? formatBytes(stats.disk_free) : "—"}
      />
      <Stat
        color="red"
        label="Reclaimable"
        value={reclaimable > 0 ? formatBytes(reclaimable) : "—"}
      />
    </div>
  );
}
