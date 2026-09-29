const DAY_MS = 86_400_000;

export const STALE_DAYS = 90;

export function downloadedAgo(secs: number): string {
  if (!secs) return "Download date unknown";
  const days = Math.floor((Date.now() / 1000 - secs) / 86400);
  const unit = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"} ago`;
  let when: string;
  if (days < 1) when = "today";
  else if (days === 1) when = "yesterday";
  else if (days < 14) when = unit(days, "day");
  else if (days < 60) when = unit(Math.floor(days / 7), "week");
  else if (days < 365) when = unit(Math.floor(days / 30), "month");
  else when = unit(Math.floor(days / 365), "year");
  return `Downloaded ${when}`;
}

export function daysSinceUsed(secs: number): number {
  const used = new Date(secs * 1000);
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const usedDay = new Date(used.getFullYear(), used.getMonth(), used.getDate()).getTime();
  return Math.max(0, Math.round((today - usedDay) / DAY_MS));
}

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

export function lastUsedLabel(secs: number): string {
  const days = daysSinceUsed(secs);
  if (days === 0) return "Used today";
  if (days === 1) return "Used yesterday";
  if (days < 7) return `Used ${days} days ago`;
  if (days < 14) return "Used last week";
  if (days < 30) return `Used ${Math.floor(days / 7)} weeks ago`;
  if (days < 60) return "Used last month";
  if (days <= STALE_DAYS) return `Used ${Math.floor(days / 30)} months ago`;
  if (days < 365) return `Last used ${plural(Math.floor(days / 30), "month")} ago`;
  return `Last used ${plural(Math.floor(days / 365), "year")} ago`;
}

export function ageTier(days: number | null): "" | " idle" | " stale" {
  if (days === null || days < 30) return "";
  return days >= 90 ? " stale" : " idle";
}

export function ageText(days: number | null): string {
  if (days === null) return "Activity unknown";
  if (days >= 30) return `${days} days idle`;
  return days === 0 ? "Active today" : `Active ${days}d ago`;
}
