import type {
  AppInfo,
  ArtifactEntry,
  InstallerFile,
  OptTask,
  ScanItem,
  PatrolRun,
  PatrolRules,
  PatrolStatus,
  ReviewItem,
} from "../lib/tauri";

const nowSecs = () => Math.floor(Date.now() / 1000);

export const scanItem = (rule_id: string, total_size: number, category = "Caches"): ScanItem => ({
  rule_id,
  category,
  label: rule_id,
  paths: [{ path: `/tmp/${rule_id}`, size: total_size, is_dir: true }],
  total_size,
});

export const artifact = (project: string, name: string, size: number, idleDays: number | null): ArtifactEntry => ({
  project_name: project,
  project_path: `/Users/me/Projects/${project}`,
  artifact_type: name,
  artifact_path: `/Users/me/Projects/${project}/${name}`,
  size,
  is_recent: idleDays !== null && idleDays < 7,
  last_modified_secs: idleDays === null ? 0 : nowSecs() - idleDays * 86400 - 60,
});

export const installer = (name: string, size: number): InstallerFile => ({
  name,
  path: `/Users/me/Downloads/${name}`,
  extension: name.split(".").pop() ?? "",
  size,
  modified_secs: nowSecs(),
});

export const app = (name: string, size = 100): AppInfo => ({
  bundle_id: `com.example.${name.toLowerCase()}`,
  name,
  version: "1.0",
  path: `/Applications/${name}.app`,
  size,
  is_system: false,
  is_data_sensitive: false,
  brew_cask: null,
  last_used_secs: null,
});

export const optTask = (id: string): OptTask => ({
  id,
  name: id,
  description: "",
  command: "",
  needs_admin: false,
  warning: null,
});

export const reviewItem = (id: string, overrides: Partial<ReviewItem> = {}): ReviewItem => ({
  id,
  name: id,
  details: "",
  size: 1000,
  score: 60,
  user_data: false,
  data_loss: null,
  found_at: nowSecs() - 3600,
  safe: false,
  ...overrides,
});

export const patrolRun = (overrides: Partial<PatrolRun> = {}): PatrolRun => ({
  started_at: nowSecs() - 3 * 3600 - 30,
  finished_at: nowSecs() - 3 * 3600,
  trigger: "schedule",
  cleaned: [],
  freed: 0,
  review_count: 0,
  error: null,
  ...overrides,
});

export const patrolRules = (overrides: Partial<PatrolRules> = {}): PatrolRules => ({
  enabled: true,
  frequency: "daily",
  low_gb: 20,
  critical_gb: 5,
  safe_action: "auto",
  review_action: "notify",
  data_action: "notify",
  ...overrides,
});

export const patrolStatus = ({ rules, ...overrides }: Partial<PatrolStatus> = {}): PatrolStatus => ({
  enabled: true,
  auto_clean: true,
  running: false,
  last_patrol_at: nowSecs() - 3 * 3600,
  next_patrol_at: nowSecs() + 6 * 3600,
  freed_total: 0,
  freed_last: 0,
  pending_review: [],
  history: [],
  ...overrides,
  rules: patrolRules({ enabled: overrides.enabled ?? true, ...rules }),
});

/** The patrol engine sends epoch milliseconds; the tauri.ts wrappers convert to seconds. */
export const toWireRun = (run: PatrolRun): PatrolRun => ({
  ...run,
  started_at: run.started_at * 1000,
  finished_at: run.finished_at * 1000,
});

export const toWireStatus = (status: PatrolStatus): PatrolStatus => ({
  ...status,
  last_patrol_at: status.last_patrol_at == null ? null : status.last_patrol_at * 1000,
  next_patrol_at: status.next_patrol_at == null ? null : status.next_patrol_at * 1000,
  pending_review: status.pending_review.map((i) => ({ ...i, found_at: i.found_at * 1000 })),
  history: status.history.map(toWireRun),
});

export function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
