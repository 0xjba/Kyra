import { openUrl } from "@tauri-apps/plugin-opener";
import { formatSize } from "./format";

const MAX_URL_LENGTH = 2000;
const BASE_URL = "https://chatgpt.com/?q=";

/**
 * Opens ChatGPT with a prefilled prompt about the user's scan results.
 */
function openChatGpt(prompt: string) {
  const url = `${BASE_URL}${encodeURIComponent(prompt)}`;
  openUrl(url).catch(() => {});
}

function urlLength(prompt: string) {
  return BASE_URL.length + encodeURIComponent(prompt).length;
}

/**
 * Check if the Clean prompt would exceed the URL limit.
 */
export function canAskAiClean(
  items: { rule_id: string; category: string; label: string; total_size: number }[],
  selectedIds: Set<string>,
): boolean {
  const selected = items.filter((i) => selectedIds.has(i.rule_id) && i.total_size > 0);
  if (selected.length === 0) return false;
  const totalSize = selected.reduce((s, i) => s + i.total_size, 0);
  const itemList = selected
    .map((i) => `- ${i.label} (${i.category}) — ${formatSize(i.total_size)}`)
    .join("\n");
  const prompt = `I'm using a macOS cleaner app called Kyra. It found the following items on my Mac that can be cleaned. Total: ${formatSize(totalSize)}.\n\n${itemList}`;
  return urlLength(prompt) <= MAX_URL_LENGTH;
}

/**
 * Clean module: ask AI about selected items to clean.
 */
export function askAiClean(
  items: { rule_id: string; category: string; label: string; total_size: number }[],
  selectedIds: Set<string>,
) {
  const selected = items.filter((i) => selectedIds.has(i.rule_id) && i.total_size > 0);
  if (selected.length === 0) return;

  const totalSize = selected.reduce((s, i) => s + i.total_size, 0);
  const itemList = selected
    .map((i) => `- ${i.label} (${i.category}) — ${formatSize(i.total_size)}`)
    .join("\n");

  const prompt = `I'm using a macOS cleaner app called Kyra. It found the following items on my Mac that can be cleaned. Total: ${formatSize(totalSize)}.

${itemList}

For each item, briefly explain:
1. What it is
2. Is it safe to delete?
3. Will deleting it break anything or slow down any app?
4. Will it be regenerated automatically?

Be concise and practical. Flag anything I should keep.`;

  openChatGpt(prompt);
}

/**
 * Check if the Prune prompt would exceed the URL limit.
 */
export function canAskAiPrune(
  artifacts: { project_name: string; artifact_type: string; artifact_path: string; size: number }[],
  selectedPaths: Set<string>,
): boolean {
  const selected = artifacts.filter((a) => selectedPaths.has(a.artifact_path));
  if (selected.length === 0) return false;
  const byType = new Map<string, { count: number; size: number; examples: string[] }>();
  for (const a of selected) {
    const entry = byType.get(a.artifact_type) || { count: 0, size: 0, examples: [] };
    entry.count++;
    entry.size += a.size;
    if (entry.examples.length < 3) entry.examples.push(a.project_name);
    byType.set(a.artifact_type, entry);
  }
  const typeList = [...byType.entries()]
    .map(([type, info]) => `- ${type}: ${info.count} items, ${formatSize(info.size)} (e.g. ${info.examples.join(", ")})`)
    .join("\n");
  const prompt = `Kyra found ${selected.length} items:\n\n${typeList}`;
  return urlLength(prompt) <= MAX_URL_LENGTH;
}

/**
 * Prune module: ask AI about selected developer artifacts.
 */
export function askAiPrune(
  artifacts: { project_name: string; artifact_type: string; artifact_path: string; size: number }[],
  selectedPaths: Set<string>,
) {
  const selected = artifacts.filter((a) => selectedPaths.has(a.artifact_path));
  if (selected.length === 0) return;

  const totalSize = selected.reduce((s, a) => s + a.size, 0);

  // Group by type for a cleaner prompt
  const byType = new Map<string, { count: number; size: number; examples: string[] }>();
  for (const a of selected) {
    const entry = byType.get(a.artifact_type) || { count: 0, size: 0, examples: [] };
    entry.count++;
    entry.size += a.size;
    if (entry.examples.length < 3) entry.examples.push(a.project_name);
    byType.set(a.artifact_type, entry);
  }

  const typeList = [...byType.entries()]
    .sort((a, b) => b[1].size - a[1].size)
    .map(([type, info]) => {
      const examples = info.examples.join(", ");
      return `- ${type}: ${info.count} items, ${formatSize(info.size)} (e.g. ${examples})`;
    })
    .join("\n");

  const prompt = `I'm using a macOS app called Kyra to clean developer artifacts from my projects. It found ${selected.length} items totaling ${formatSize(totalSize)}:

${typeList}

For each artifact type, briefly explain:
1. What it is and why it's large
2. Is it safe to delete?
3. What happens after deletion? (e.g. will npm install recreate it?)
4. Any cases where I should NOT delete it?

Be concise and practical.`;

  openChatGpt(prompt);
}

/**
 * Check if the Optimize prompt would exceed the URL limit.
 */
export function canAskAiOptimize(
  tasks: { id: string; name: string; description: string; warning: string | null; needs_admin: boolean }[],
  enabledIds: Set<string>,
): boolean {
  const selected = tasks.filter((t) => enabledIds.has(t.id));
  if (selected.length === 0) return false;
  const taskList = selected
    .map((t) => {
      let line = `- ${t.name}: ${t.description}`;
      if (t.warning) line += ` (Warning: ${t.warning})`;
      if (t.needs_admin) line += ` [Requires admin]`;
      return line;
    })
    .join("\n");
  const prompt = `Kyra optimization tasks:\n\n${taskList}`;
  return urlLength(prompt) <= MAX_URL_LENGTH;
}

/**
 * Optimize module: ask AI about selected optimization tasks.
 */
export function askAiOptimize(
  tasks: { id: string; name: string; description: string; warning: string | null; needs_admin: boolean }[],
  enabledIds: Set<string>,
) {
  const selected = tasks.filter((t) => enabledIds.has(t.id));
  if (selected.length === 0) return;

  const taskList = selected
    .map((t) => {
      let line = `- ${t.name}: ${t.description}`;
      if (t.warning) line += ` (Warning: ${t.warning})`;
      if (t.needs_admin) line += ` [Requires admin]`;
      return line;
    })
    .join("\n");

  const prompt = `I'm using a macOS optimizer app called Kyra. I'm about to run these optimization tasks on my Mac:

${taskList}

For each task, briefly explain:
1. What it actually does under the hood
2. Is it safe to run?
3. Will it affect any running apps or require a restart?
4. How much performance improvement can I realistically expect?

Be concise and practical. Flag anything risky.`;

  openChatGpt(prompt);
}
