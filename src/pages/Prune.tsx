import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { FolderOpen, Check } from "lucide-react";
import { usePruneStore, idleDaysByProject } from "../stores/pruneStore";
import { formatSize } from "../utils/format";
import { ageTier, ageText } from "../utils/relativeTime";
import { pickFolder, listenPruneScanProgress, type ArtifactEntry } from "../lib/tauri";
import { pickEquivalenceCard, type EquivalenceCard } from "../utils/equivalenceCards";
import DeleteConfirmDialog from "../components/DeleteConfirmDialog";
import { askAiPrune, canAskAiPrune } from "../utils/askAi";
import AskAiCoachMark from "../components/AskAiCoachMark";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import walk1 from "../assets/cat-walking/cat_walking_01.png";
import walk2 from "../assets/cat-walking/cat_walking_02.png";
import walk3 from "../assets/cat-walking/cat_walking_03.png";
import walk4 from "../assets/cat-walking/cat_walking_04.png";
import walk5 from "../assets/cat-walking/cat_walking_05.png";
import "../styles/prune.css";

const TAIL_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];
const WALK_FRAMES = [walk1, walk2, walk3, walk4, walk5];

function useFrame(count: number, ms: number): number {
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setFrame((f) => (f + 1) % count), ms);
    return () => clearInterval(id);
  }, [count, ms]);
  return frame;
}

const TYPE_COLORS: Record<string, string> = {
  "Node.js": "#2AC852",
  "Build Output": "#FD8C34",
  "Rust": "#FD4841",
  "Python": "#FDD225",
  "Next.js": "#1f5fff",
  "Nuxt.js": "#5A67F2",
  "Xcode Build": "#FF5DA2",
  "CocoaPods": "#A2845E",
  "Swift": "#13D1BB",
  "Gradle": "#22B8F0",
  "Python Virtual Env": "#8E5CF6",
  "Test Coverage": "#8E8E93",
  "Vendor Deps": "#747474",
  "Turbo Cache": "#13D1BB",
  "Parcel Cache": "#FD8C34",
  "Angular Cache": "#FD4841",
  "SvelteKit": "#FD8C34",
  "Astro Cache": "#8E5CF6",
  "Pytest Cache": "#FDD225",
  "Mypy Cache": "#FDD225",
  "Ruff Cache": "#FDD225",
  "C#/.NET Build": "#5A67F2",
  "C++ Build": "#22B8F0",
  "Expo Cache": "#1f5fff",
  "Dart Tool": "#22B8F0",
  "Nitro/Nuxt Output": "#5A67F2",
  "Tox Env": "#FDD225",
  "Nox Env": "#FDD225",
  "Maven": "#FD8C34",
  "Elixir": "#8E5CF6",
  "Elixir Deps": "#8E5CF6",
  "Haskell": "#FF5DA2",
  "OCaml": "#FDD225",
  "Ruby Bundler": "#FD4841",
  "CMake Build": "#22B8F0",
  "Bun Cache": "#2AC852",
};

const TYPE_LABELS: Record<string, string> = {
  "Node.js": "node_modules",
  "Rust": "target",
  "Build Output": "build / dist",
  "Xcode Build": "DerivedData",
  "Python Virtual Env": "venv",
  "Gradle": ".gradle",
  "Next.js": ".next",
  "Nuxt.js": ".nuxt",
  "CocoaPods": "Pods",
  "Turbo Cache": ".turbo",
  "Pytest Cache": ".pytest_cache",
  "Mypy Cache": ".mypy_cache",
  "Ruff Cache": ".ruff_cache",
  "Dart Tool": ".dart_tool",
  "Expo Cache": ".expo",
  "Swift": ".build",
};

function typeColor(type: string): string {
  return TYPE_COLORS[type] || "#8E8E93";
}

function typeLabel(type: string): string {
  return TYPE_LABELS[type] || type;
}

function tildePath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

const GB = 1024 * 1024 * 1024;

const DEFAULT_PATHS = ["~/", "~/Projects", "~/Developer", "~/Code", "~/dev", "~/src"];
const RECENT_PATHS_KEY = "kyra_prune_recent_paths";

function loadRecentPaths(): string[] {
  try {
    const stored = localStorage.getItem(RECENT_PATHS_KEY);
    return stored ? JSON.parse(stored) : [];
  } catch { return []; }
}

function saveRecentPath(path: string) {
  try {
    const recent = loadRecentPaths().filter((p) => p !== path);
    recent.unshift(path);
    localStorage.setItem(RECENT_PATHS_KEY, JSON.stringify(recent.slice(0, 6)));
  } catch { /* ignore */ }
}

function chipLabel(path: string): string {
  if (DEFAULT_PATHS.includes(path)) return path;
  const clean = path.replace(/\/+$/, "");
  const parts = clean.split("/").filter(Boolean);
  if (parts.length <= 2) return path.startsWith("/") ? `/${parts.join("/")}` : parts.join("/");
  return `.../${parts[parts.length - 1]}`;
}

const DETECTED_TYPES = [
  { label: "node_modules", color: "#2AC852" },
  { label: "target", color: "#FD4841" },
  { label: "build / dist", color: "#FD8C34" },
  { label: "DerivedData", color: "#FF5DA2" },
  { label: "venv", color: "#8E5CF6" },
  { label: ".gradle", color: "#22B8F0" },
];

function IdleView() {
  const rootPath = usePruneStore((s) => s.rootPath);
  const setRootPath = usePruneStore((s) => s.setRootPath);
  const scan = usePruneStore((s) => s.scan);
  const error = usePruneStore((s) => s.error);
  const frame = useFrame(TAIL_FRAMES.length, 220);

  const quickPaths = useMemo(() => {
    const merged = [...loadRecentPaths()];
    for (const p of DEFAULT_PATHS) {
      if (!merged.includes(p)) merged.push(p);
    }
    return merged.slice(0, 6);
  }, []);

  const handleScan = useCallback(() => {
    if (!rootPath.trim()) return;
    saveRecentPath(rootPath);
    scan();
  }, [rootPath, scan]);

  const handlePickFolder = useCallback(async () => {
    const selected = await pickFolder();
    if (selected) setRootPath(selected);
  }, [setRootPath]);

  return (
    <div className="prune-hero">
      <div className="prune-hero-cat">
        <img src={TAIL_FRAMES[frame]} alt="" />
        <div className="prune-hero-bubble">Point me at your projects.</div>
      </div>
      <div className="prune-hero-title">Find build leftovers</div>
      <div className="prune-hero-desc">
        Finds node_modules, target, dist and other build outputs inside your project folders. Usually recovers 2 to 10 GB.
      </div>

      <div className="prune-path-bar">
        <div className="prune-path-field">
          <button className="prune-path-browse" onClick={handlePickFolder} title="Choose folder">
            <FolderOpen size={14} strokeWidth={2} />
          </button>
          <input
            type="text"
            value={rootPath}
            onChange={(e) => setRootPath(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") handleScan(); }}
            placeholder="Browse or type a path to scan..."
            spellCheck={false}
          />
        </div>
        <button className="prune-scan-btn" onClick={handleScan} disabled={!rootPath.trim()}>
          Start scan
        </button>
      </div>

      <div className="prune-quick">
        {quickPaths.map((p) => (
          <button
            key={p}
            className={`prune-quick-chip${rootPath === p ? " on" : ""}`}
            onClick={() => setRootPath(p)}
            title={p}
          >
            {chipLabel(p)}
          </button>
        ))}
      </div>

      {error && <div className="prune-err">{error}</div>}

      <div className="prune-detect">
        <div className="prune-eyebrow">What gets detected</div>
        <div className="prune-detect-chips">
          {DETECTED_TYPES.map((t) => (
            <span key={t.label} className="prune-detect-chip">
              <span className="prune-detect-dot" style={{ background: t.color }} />
              {t.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

function ScanningView() {
  const rootPath = usePruneStore((s) => s.rootPath);
  const [found, setFound] = useState(0);
  const [current, setCurrent] = useState("");
  const [pos, setPos] = useState({ x: 8, dir: 1 });
  const frame = useFrame(WALK_FRAMES.length, 120);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listenPruneScanProgress((p) => {
      setFound(p.artifacts_found);
      setCurrent(p.current_path);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => { cancelled = true; if (unlisten) unlisten(); };
  }, []);

  useEffect(() => {
    const id = setInterval(() => {
      setPos(({ x, dir }) => {
        const nx = x + dir * 0.9;
        if (nx >= 92) return { x: 92, dir: -1 };
        if (nx <= 8) return { x: 8, dir: 1 };
        return { x: nx, dir };
      });
    }, 120);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="prune-scan">
      <div className="prune-eyebrow prune-scan-eyebrow">Sniffing around</div>
      <div className="prune-scan-count">
        {found}
        <span className="prune-scan-unit">{found === 1 ? "folder found" : "folders found"}</span>
      </div>
      <div className="prune-scan-track-wrap">
        <img
          className="prune-scan-cat"
          src={WALK_FRAMES[frame]}
          alt=""
          style={{ left: `${pos.x}%`, transform: pos.dir < 0 ? "scaleX(-1)" : undefined }}
        />
        <div className="prune-scan-track">
          <div className="prune-scan-glow" style={{ left: `${pos.x}%` }} />
        </div>
      </div>
      <div className="prune-scan-path">{tildePath(current || rootPath)}</div>
    </div>
  );
}

interface ProjectSeg {
  type: string;
  label: string;
  color: string;
  size: number;
}

interface ProjectGroup {
  key: string;
  name: string;
  path: string;
  size: number;
  segs: ProjectSeg[];
  paths: string[];
  days: number | null;
}

type AgeFilter = 0 | 30 | 90;

const AGE_FILTERS: { min: AgeFilter; label: string }[] = [
  { min: 0, label: "All" },
  { min: 30, label: "30d+" },
  { min: 90, label: "90d+" },
];

function ListView() {
  const artifacts = usePruneStore((s) => s.artifacts);
  const selectedPaths = usePruneStore((s) => s.selectedPaths);
  const error = usePruneStore((s) => s.error);
  const reset = usePruneStore((s) => s.reset);
  const prune = usePruneStore((s) => s.prune);

  const [off, setOff] = useState<Set<string>>(new Set());
  const [minAge, setMinAge] = useState<AgeFilter>(0);
  const ageStash = useRef<Set<string>>(new Set());
  const [showConfirm, setShowConfirm] = useState(false);
  const frame = useFrame(TAIL_FRAMES.length, 220);

  const setSelected = (next: Set<string>) => usePruneStore.setState({ selectedPaths: next });

  const types = useMemo(() => {
    const map = new Map<string, number>();
    for (const a of artifacts) map.set(a.artifact_type, (map.get(a.artifact_type) || 0) + a.size);
    return Array.from(map.entries())
      .sort((a, b) => b[1] - a[1])
      .map(([type, size]) => ({ type, size, label: typeLabel(type), color: typeColor(type) }));
  }, [artifacts]);

  const idle = useMemo(() => idleDaysByProject(artifacts), [artifacts]);
  const ageOk = (projectPath: string, min: AgeFilter = minAge) =>
    min === 0 || (idle.get(projectPath) ?? -1) >= min;

  const visible = useMemo(
    () =>
      artifacts.filter(
        (a) => !off.has(a.artifact_type) && (minAge === 0 || (idle.get(a.project_path) ?? -1) >= minAge),
      ),
    [artifacts, off, idle, minAge],
  );

  const projects = useMemo(() => {
    const map = new Map<string, { name: string; items: ArtifactEntry[] }>();
    for (const a of visible) {
      const g = map.get(a.project_path) || { name: a.project_name, items: [] };
      g.items.push(a);
      map.set(a.project_path, g);
    }
    const out: ProjectGroup[] = [];
    for (const [key, g] of map) {
      const byType = new Map<string, number>();
      for (const a of g.items) byType.set(a.artifact_type, (byType.get(a.artifact_type) || 0) + a.size);
      const segs = Array.from(byType.entries())
        .sort((a, b) => b[1] - a[1])
        .map(([type, size]) => ({ type, size, label: typeLabel(type), color: typeColor(type) }));
      const size = g.items.reduce((s, a) => s + a.size, 0);
      if (size <= 0) continue;
      out.push({
        key,
        name: g.name,
        path: tildePath(key),
        size,
        segs,
        paths: g.items.map((a) => a.artifact_path),
        days: idle.get(key) ?? null,
      });
    }
    return out.sort((a, b) => b.size - a.size);
  }, [visible, idle]);

  const total = projects.reduce((s, p) => s + p.size, 0);
  const staleCount = projects.filter((p) => (p.days ?? -1) >= 90).length;
  const isOn = (p: ProjectGroup) => p.paths.some((x) => selectedPaths.has(x));
  const selProjects = projects.filter(isOn);
  const selectedSize = artifacts
    .filter((a) => selectedPaths.has(a.artifact_path))
    .reduce((s, a) => s + a.size, 0);
  const allSelected = visible.length > 0 && visible.every((a) => selectedPaths.has(a.artifact_path));

  const toggleProject = (p: ProjectGroup) => {
    const next = new Set(selectedPaths);
    if (isOn(p)) p.paths.forEach((x) => next.delete(x));
    else p.paths.forEach((x) => next.add(x));
    setSelected(next);
  };

  const toggleType = (type: string) => {
    const nextOff = new Set(off);
    const next = new Set(selectedPaths);
    if (off.has(type)) {
      nextOff.delete(type);
      const byProject = new Map<string, ArtifactEntry[]>();
      for (const a of artifacts) {
        const list = byProject.get(a.project_path) || [];
        list.push(a);
        byProject.set(a.project_path, list);
      }
      for (const [projectPath, list] of byProject) {
        if (!ageOk(projectPath)) continue;
        const others = list.filter((a) => a.artifact_type !== type && !off.has(a.artifact_type));
        const projectOn = others.length === 0 || others.some((a) => selectedPaths.has(a.artifact_path));
        if (projectOn) list.filter((a) => a.artifact_type === type).forEach((a) => next.add(a.artifact_path));
      }
    } else {
      nextOff.add(type);
      artifacts.filter((a) => a.artifact_type === type).forEach((a) => next.delete(a.artifact_path));
    }
    setOff(nextOff);
    setSelected(next);
  };

  const pickAge = (min: AgeFilter) => {
    if (min === minAge) return;
    const next = new Set(selectedPaths);
    const stash = ageStash.current;
    for (const a of artifacts) {
      const was = ageOk(a.project_path);
      const now = ageOk(a.project_path, min);
      if (was && !now && next.has(a.artifact_path)) {
        next.delete(a.artifact_path);
        stash.add(a.artifact_path);
      } else if (!was && now && stash.has(a.artifact_path)) {
        stash.delete(a.artifact_path);
        if (!off.has(a.artifact_type)) next.add(a.artifact_path);
      }
    }
    setMinAge(min);
    setSelected(next);
  };

  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(visible.map((a) => a.artifact_path)));
  };

  if (artifacts.length === 0) {
    return (
      <div className="prune-hero">
        <div className="prune-hero-cat">
          <img src={TAIL_FRAMES[frame]} alt="" />
        </div>
        <div className="prune-hero-title">Nothing to prune</div>
        <div className="prune-hero-desc">
          No developer artifacts were found in this folder. Your projects are already clean.
        </div>
        <button className="prune-pill" onClick={reset} style={{ marginTop: 4 }}>New scan</button>
      </div>
    );
  }

  const selCount = selectedPaths.size;
  const askAiOk = selCount > 0 && canAskAiPrune(artifacts, selectedPaths);

  return (
    <>
      <div className="prune-head">
        <img className="prune-head-cat" src={TAIL_FRAMES[frame]} alt="" />
        <div className="prune-head-main">
          <div className="prune-head-line">
            <span className="prune-head-total">{formatSize(total)}</span>
            <span className="prune-head-sub">
              of build leftovers in {projects.length} project{projects.length === 1 ? "" : "s"}
              {staleCount > 0 && ` · ${staleCount} untouched for 90+ days`}
            </span>
          </div>
          <div className="prune-head-bar">
            {types.map((t) => (
              <span
                key={t.type}
                onClick={() => toggleType(t.type)}
                title={`${t.label} · ${formatSize(t.size)}`}
                style={{ flex: t.size, background: t.color, opacity: off.has(t.type) ? 0.35 : 1 }}
              />
            ))}
          </div>
        </div>
        <div className="prune-head-actions" role="radiogroup" aria-label="Idle time">
          {AGE_FILTERS.map((f) => (
            <button
              key={f.min}
              role="radio"
              aria-checked={minAge === f.min}
              className={`prune-seg-btn${minAge === f.min ? " on" : ""}`}
              onClick={() => pickAge(f.min)}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      <div className="prune-types">
        {types.map((t) => (
          <button
            key={t.type}
            className={`prune-type-chip${off.has(t.type) ? " off" : ""}`}
            onClick={() => toggleType(t.type)}
          >
            <span className="prune-type-sw" style={{ background: t.color }} />
            {t.label}
            <span className="prune-type-size">{formatSize(t.size)}</span>
          </button>
        ))}
      </div>

      <div className="prune-bento-scroll">
        <div className="prune-bento">
          {projects.map((p, i) => {
            const on = isOn(p);
            const hero = i === 0 && projects.length >= 3 && (p.size >= 2.5 * GB || p.size >= total * 0.3);
            const wide = !hero && p.size >= 1.5 * GB;
            const dom = p.segs[0].color;
            const tier = ageTier(p.days);
            const months = p.days === null ? 0 : Math.min(5, Math.floor(p.days / 30));
            return (
              <div
                key={p.key}
                className={`prune-tile${on ? " on" : ""}${hero ? " hero" : wide ? " wide" : ""}`}
                style={{ "--dom": dom } as React.CSSProperties}
                onClick={() => toggleProject(p)}
                title={p.path}
              >
                <div className="prune-tile-top">
                  <div className="prune-tile-id">
                    <div className="prune-tile-name">{p.name}</div>
                    <div className="prune-tile-path">{p.path}</div>
                  </div>
                  <span className={`prune-tile-check${on ? " on" : ""}`}>
                    {on && <Check size={11} strokeWidth={3} />}
                  </span>
                </div>
                <div className="prune-tile-bottom">
                  <div className="prune-tile-size">{formatSize(p.size)}</div>
                  <div className={`prune-tile-meta${tier}`}>
                    <div className="prune-tile-dots">
                      {[0, 1, 2, 3, 4].map((d) => (
                        <span key={d} className={d < months ? "on" : undefined} />
                      ))}
                    </div>
                    <span className="prune-tile-age">{ageText(p.days)}</span>
                  </div>
                </div>
                <div className="prune-tile-bar">
                  {p.segs.map((g) => (
                    <span
                      key={g.type}
                      title={`${g.label} · ${formatSize(g.size)}`}
                      style={{ flex: g.size, background: g.color }}
                    />
                  ))}
                </div>
                {hero && (
                  <div className="prune-tile-chips">
                    {p.segs.map((g) => (
                      <span key={g.type}>{g.label} · {formatSize(g.size)}</span>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>

      <div className="prune-foot">
        {error ? (
          <span className="prune-foot-info prune-foot-err">{error}</span>
        ) : (
          <span className="prune-foot-info">
            {selProjects.length} of {projects.length} selected · your source code is never touched
          </span>
        )}
        <button className="prune-ai-btn prune-foot-first" onClick={toggleAll} disabled={visible.length === 0}>
          {allSelected ? "Deselect all" : "Select all"}
        </button>
        <button className="prune-ai-btn" onClick={reset}>New scan</button>
        <span
          className="tooltip-wrap"
          data-tooltip={selCount > 0 && !askAiOk ? "Select fewer items to Ask AI" : undefined}
        >
          <AskAiCoachMark />
          <button
            className="prune-ai-btn"
            disabled={!askAiOk}
            onClick={() => askAiPrune(artifacts, selectedPaths)}
          >
            Ask AI
          </button>
        </span>
        <button
          className="prune-go-btn"
          disabled={selCount === 0}
          onClick={() => setShowConfirm(true)}
        >
          {selCount > 0 ? `Prune ${formatSize(selectedSize)}` : "Prune"}
        </button>
      </div>

      <DeleteConfirmDialog
        visible={showConfirm}
        title={`Prune ${selCount} artifact${selCount > 1 ? "s" : ""} (${formatSize(selectedSize)})?`}
        onConfirm={() => { setShowConfirm(false); prune(); }}
        onCancel={() => setShowConfirm(false)}
      />
    </>
  );
}

/* ── Confetti Particle ── */
const CONFETTI_COLORS = [
  "rgba(31,95,255,0.6)",
  "rgba(31,95,255,0.4)",
  "rgba(31,95,255,0.3)",
  "rgba(253, 72, 65, 0.35)",    // red muted
  "rgba(42, 200, 82, 0.35)",    // green muted
  "rgba(31, 95, 255, 0.3)",    // blue muted
  "rgba(253, 210, 37, 0.3)",    // yellow muted
  "rgba(142, 92, 246, 0.3)",    // purple muted
];

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  rotation: number;
  rotationSpeed: number;
  size: number;
  color: string;
  opacity: number;
  life: number;
  maxLife: number;
}

function Confetti({ active }: { active: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!active) return;
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    canvas.width = canvas.offsetWidth * 2;
    canvas.height = canvas.offsetHeight * 2;
    ctx.scale(2, 2);

    const w = canvas.offsetWidth;
    const h = canvas.offsetHeight;

    // Create 20 particles from center-ish
    const particles: Particle[] = [];
    for (let i = 0; i < 20; i++) {
      particles.push({
        x: w / 2 + (Math.random() - 0.5) * 60,
        y: h * 0.3,
        vx: (Math.random() - 0.5) * 3,
        vy: -(Math.random() * 2 + 1),
        rotation: Math.random() * 360,
        rotationSpeed: (Math.random() - 0.5) * 8,
        size: Math.random() * 4 + 2,
        color: CONFETTI_COLORS[Math.floor(Math.random() * CONFETTI_COLORS.length)],
        opacity: 1,
        life: 0,
        maxLife: 1600 + Math.random() * 1800,  // 1.6–3.4s
      });
    }

    let animId: number;
    let lastTime = performance.now();

    function animate(now: number) {
      const dt = Math.min(now - lastTime, 32);
      lastTime = now;

      ctx!.clearRect(0, 0, w, h);

      let alive = 0;
      for (const p of particles) {
        p.life += dt;
        if (p.life > p.maxLife) continue;
        alive++;

        const progress = p.life / p.maxLife;
        p.vy += 0.03;  // gravity
        p.x += p.vx;
        p.y += p.vy;
        p.rotation += p.rotationSpeed;
        p.opacity = 1 - Math.pow(progress, 2);

        ctx!.save();
        ctx!.translate(p.x, p.y);
        ctx!.rotate((p.rotation * Math.PI) / 180);
        ctx!.globalAlpha = p.opacity;
        ctx!.fillStyle = p.color;
        ctx!.fillRect(-p.size / 2, -p.size / 2, p.size, p.size * 0.6);
        ctx!.restore();
      }

      if (alive > 0) {
        animId = requestAnimationFrame(animate);
      }
    }

    animId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animId);
  }, [active]);

  if (!active) return null;
  return <canvas ref={canvasRef} className="prune-confetti" />;
}

/* ── SSD Icon for milestone cards ── */
function SsdIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="6" width="18" height="12" rx="2" />
      <line x1="7" y1="10" x2="7" y2="14" />
      <line x1="11" y1="10" x2="11" y2="14" />
      <line x1="15" y1="10" x2="15" y2="14" />
    </svg>
  );
}

/* ── Pruning View ── */
function PruningView() {
  const phase = usePruneStore((s) => s.phase);
  const progress = usePruneStore((s) => s.progress);
  const artifacts = usePruneStore((s) => s.artifacts);
  const selectedPaths = usePruneStore((s) => s.selectedPaths);
  const dismissDone = usePruneStore((s) => s.dismissDone);
  const isDone = phase === "done";

  // Staggered animation state
  const [showCard, setShowCard] = useState(false);
  const [showChips, setShowChips] = useState(false);
  const [showDone, setShowDone] = useState(false);
  const [showConfetti, setShowConfetti] = useState(false);
  const [chipsExpanded, setChipsExpanded] = useState(false);

  // Pick equivalence card once when done
  const cardRef = useRef<EquivalenceCard | null>(null);
  if (isDone && !cardRef.current && progress) {
    cardRef.current = pickEquivalenceCard(progress.bytes_freed);
  }

  // Reset animation state when leaving done
  useEffect(() => {
    if (!isDone) {
      setShowCard(false);
      setShowChips(false);
      setShowDone(false);
      setShowConfetti(false);
      setChipsExpanded(false);
      cardRef.current = null;
      return;
    }

    // Staggered reveal
    const t1 = setTimeout(() => { setShowConfetti(true); setShowCard(true); }, 500);
    const t2 = setTimeout(() => setShowChips(true), 800);
    const t3 = setTimeout(() => setShowDone(true), 1050);

    return () => { clearTimeout(t1); clearTimeout(t2); clearTimeout(t3); };
  }, [isDone]);

  const percent =
    isDone
      ? 100
      : progress && progress.items_total > 0
        ? Math.round((progress.items_done / progress.items_total) * 100)
        : 0;

  // SVG ring dimensions
  const ringSize = 120;
  const strokeWidth = 6;
  const radius = (ringSize - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference - (percent / 100) * circumference;

  // Extract folder name from current_item path for display
  const currentLabel = progress?.current_item
    ? `Removing ${progress.current_item.split("/").filter(Boolean).pop() || progress.current_item}...`
    : "Starting...";

  // Breakdown chips — group selected items by artifact_type
  const breakdownChips = useMemo(() => {
    if (!isDone) return [];
    const selected = artifacts.filter((a) => selectedPaths.has(a.artifact_path));
    const map = new Map<string, { count: number; size: number }>();
    for (const a of selected) {
      const existing = map.get(a.artifact_type) || { count: 0, size: 0 };
      existing.count++;
      existing.size += a.size;
      map.set(a.artifact_type, existing);
    }
    return Array.from(map.entries())
      .sort((a, b) => b[1].size - a[1].size)
      .map(([type, { count, size }]) => ({ type, count, size }));
  }, [isDone, artifacts, selectedPaths]);

  const visibleChips = chipsExpanded ? breakdownChips : breakdownChips.slice(0, 3);
  const hiddenCount = breakdownChips.length - 3;

  const card = cardRef.current;

  return (
    <div className={`centered${isDone ? " prune-done" : ""}`}>
      <Confetti active={showConfetti} />

      {/* Circular progress ring */}
      <div className="prune-ring-wrap">
        <svg
          className="prune-ring-svg"
          width={ringSize}
          height={ringSize}
          viewBox={`0 0 ${ringSize} ${ringSize}`}
        >
          <defs>
            <linearGradient id="ring-glass" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="#4f8bff" />
              <stop offset="50%" stopColor="#3573e6" />
              <stop offset="100%" stopColor="#1f5fff" />
            </linearGradient>
            <linearGradient id="ring-glass-done" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stopColor="#2AC852" />
              <stop offset="50%" stopColor="#22b347" />
              <stop offset="100%" stopColor="#1a9e3e" />
            </linearGradient>
            <filter id="ring-glow">
              <feGaussianBlur stdDeviation="3" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>
          {/* Background track */}
          <circle
            cx={ringSize / 2}
            cy={ringSize / 2}
            r={radius}
            fill="none"
            stroke="rgba(255,255,255,0.6)"
            strokeWidth={strokeWidth}
          />
          {/* Filled arc — glass gradient */}
          <circle
            cx={ringSize / 2}
            cy={ringSize / 2}
            r={radius}
            fill="none"
            stroke={isDone ? "url(#ring-glass-done)" : "url(#ring-glass)"}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            className="prune-ring-fill"
            filter={isDone ? "url(#ring-glow)" : undefined}
          />
        </svg>
        {isDone ? (
          <Check size={32} strokeWidth={2.5} className="prune-ring-check" />
        ) : (
          <span className="prune-ring-percent">{percent}%</span>
        )}
      </div>

      {/* Status text */}
      <div className="prune-ring-freed">
        {progress ? formatSize(progress.bytes_freed) : "0 B"} reclaimed
      </div>

      <div className="prune-ring-current">
        {isDone
          ? `${progress ? progress.items_total : 0} items removed`
          : currentLabel}
      </div>

      {/* Layer 2: Equivalence card (slides up) */}
      {isDone && card && (
        <div className={`prune-equiv-card${showCard ? " visible" : ""}`}>
          <div className="prune-equiv-icon">
            {card.isMilestone ? <SsdIcon /> : <span className="prune-equiv-emoji">{card.emoji}</span>}
          </div>
          <div className="prune-equiv-text">
            <div className="prune-equiv-title">{card.title}</div>
            <div className="prune-equiv-desc">{card.description}</div>
          </div>
        </div>
      )}

      {/* Layer 3: Breakdown chips (slides up) */}
      {isDone && breakdownChips.length > 0 && (
        <div className={`prune-breakdown-chips${showChips ? " visible" : ""}`}>
          {visibleChips.map((chip) => (
            <span key={chip.type} className="prune-breakdown-chip">
              {chip.count} {chip.type.toLowerCase()} · {formatSize(chip.size)}
            </span>
          ))}
          {hiddenCount > 0 && !chipsExpanded && (
            <button
              className="prune-breakdown-chip prune-breakdown-more"
              onClick={() => setChipsExpanded(true)}
            >
              +{hiddenCount} more
            </button>
          )}
        </div>
      )}

      {/* Done button (slides up) */}
      {isDone && (
        <button
          className={`btn prune-done-btn${showDone ? " visible" : ""}`}
          onClick={dismissDone}
        >
          Done
        </button>
      )}
    </div>
  );
}

/* ── Main ── */
export default function Prune() {
  const phase = usePruneStore((s) => s.phase);

  return (
    <div className="prune-container">
      {phase === "idle" && <IdleView />}
      {phase === "scanning" && <ScanningView />}
      {phase === "list" && <ListView />}
      {(phase === "pruning" || phase === "done") && <PruningView />}
    </div>
  );
}
