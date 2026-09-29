import type { PatrolRun, PatrolStatus } from "../lib/tauri";
import { formatSize } from "./format";

const HOUR = 3600;
const DAY = 86400;

export function agoLabel(secs: number, nowMs = Date.now()): string {
  const diff = Math.max(0, Math.floor(nowMs / 1000 - secs));
  if (diff < 60) return "just now";
  if (diff < HOUR) return `${Math.floor(diff / 60)}m ago`;
  if (diff < DAY) return `${Math.floor(diff / HOUR)}h ago`;
  if (diff < 2 * DAY) return "yesterday";
  return `${Math.floor(diff / DAY)}d ago`;
}

function partOfDay(hour: number): string {
  if (hour < 12) return "morning";
  if (hour < 18) return "afternoon";
  return "evening";
}

export function nextPatrolLabel(secs: number, nowMs = Date.now()): string {
  const diff = secs - nowMs / 1000;
  if (diff <= 60) return "next any minute";
  if (diff < HOUR) return `next in ${Math.round(diff / 60)} min`;
  const at = new Date(secs * 1000);
  const now = new Date(nowMs);
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  if (at.toDateString() === now.toDateString()) {
    if (at.getHours() >= 18) return "next around tonight";
    return `next around ${at.toLocaleTimeString("en-US", { hour: "numeric" })}`;
  }
  if (at.toDateString() === tomorrow.toDateString()) return `next tomorrow ${partOfDay(at.getHours())}`;
  return `next ${at.toLocaleDateString("en-US", { weekday: "long" })}`;
}

export function patrolStatusLine(status: PatrolStatus, nowMs = Date.now()): string {
  if (status.running) return "Checking now…";
  const ago = status.last_patrol_at ? agoLabel(status.last_patrol_at, nowMs) : null;
  if (!status.enabled) return ago ? `Paused · last run ${ago}` : "Paused until you resume it";
  const head = ago ? `Last run ${ago}` : "Hasn't run yet";
  return status.next_patrol_at ? `${head} · ${nextPatrolLabel(status.next_patrol_at, nowMs)}` : head;
}

function listNames(names: string[]): string {
  if (names.length <= 2) return names.join(", ");
  return `${names.slice(0, 2).join(", ")} and ${names.length - 2} more`;
}

export function runSummary(run: PatrolRun): { title: string; detail: string } {
  if (run.error) return { title: "Didn't finish", detail: run.error };
  const review = run.review_count > 0 ? `${run.review_count} for you to review` : "";
  if (run.cleaned.length > 0) {
    const cleaned = `Cleaned ${listNames(run.cleaned.map((c) => c.name))}`;
    return { title: `Freed ${formatSize(run.freed)}`, detail: review ? `${cleaned} · ${review}` : cleaned };
  }
  if (review) return { title: "Nothing cleaned", detail: `Found ${review}` };
  return { title: "Found nothing", detail: "Everything was already tidy" };
}

function sentence(text: string): string {
  const t = text.trim();
  return /[.!?]$/.test(t) ? t : `${t}.`;
}

export function dataLossCopy(losses: string[], useTrash: boolean): string {
  const tail = useTrash ? "It goes to the Trash, so you can still restore it." : "This can't be undone.";
  return [...losses.map(sentence), tail].join(" ");
}
