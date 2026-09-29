import type { SystemStats } from "../lib/tauri";
import type { Summaries } from "../stores/summaryStore";
import { formatSize } from "./format";

const GB = 1024 * 1024 * 1024;

export interface HomeSpeech {
  a: string;
  b: string;
  c: string;
  cta: string;
  to: string;
}

export function homeSpeech(summaries: Summaries, stats: SystemStats | null): HomeSpeech {
  const found = [
    { route: "/prune", bytes: summaries.prune?.bytes ?? 0 },
    { route: "/installers", bytes: summaries.installers?.bytes ?? 0 },
    { route: "/clean", bytes: summaries.clean?.bytes ?? 0 },
  ];
  const reclaimable = found.reduce((sum, f) => sum + f.bytes, 0);
  const biggest = found.reduce((a, b) => (b.bytes > a.bytes ? b : a));
  const hasScanned = Boolean(summaries.prune || summaries.installers || summaries.clean);
  const diskUsed = stats ? stats.disk_total - stats.disk_free : 0;

  if (reclaimable > 0) {
    return { a: "I sniffed out ", b: formatSize(reclaimable), c: " you can let go of. Want me to handle it?", cta: "Review it all", to: biggest.route };
  }
  if (hasScanned) {
    return { a: "All tidy. ", b: stats ? `${Math.round(diskUsed / GB)} GB` : "Your Mac", c: " used and nothing stale. I'll keep watching.", cta: "Scan anyway", to: "/clean" };
  }
  return { a: "Want me to sniff out some ", b: "clutter", c: "? Start with a quick scan.", cta: "Start scanning", to: "/clean" };
}

export function tileValue(bytes: number | undefined): string | undefined {
  return bytes === undefined ? undefined : bytes > 0 ? formatSize(bytes) : "Tidy";
}

export function daysSinceOptimized(optimizedAt: number | undefined, now = Date.now()): number | null {
  return optimizedAt !== undefined ? Math.floor((now - optimizedAt) / 86_400_000) : null;
}
