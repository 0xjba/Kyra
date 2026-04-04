import { useMemo, useRef, useEffect, useState } from "react";
import squarify from "squarify";
import type { DirNode } from "../lib/tauri";
import { formatSize } from "../utils/format";

interface TreemapProps {
  node: DirNode;
  onDrillIn: (node: DirNode) => void;
}

const GAP = 4;

export default function Treemap({ node, onDrillIn }: TreemapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [dims, setDims] = useState({ w: 600, h: 400 });

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let rafId: number;
    const observer = new ResizeObserver((entries) => {
      cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        for (const entry of entries) {
          setDims({
            w: entry.contentRect.width,
            h: entry.contentRect.height,
          });
        }
      });
    });
    observer.observe(el);
    return () => {
      cancelAnimationFrame(rafId);
      observer.disconnect();
    };
  }, []);

  const rects = useMemo(() => {
    if (dims.w < 10 || dims.h < 10) return [];

    const totalSize = node.size || 1;
    const container = { x0: 0, y0: 0, x1: dims.w, y1: dims.h };
    const MIN_W = 80;
    const MIN_H = 36;

    // Start with all non-empty children sorted descending
    let candidates = node.children
      .filter((c) => c.size > 0)
      .sort((a, b) => b.size - a.size);

    // Iteratively layout and remove cards that are too small,
    // then re-layout so remaining cards fill the full space
    for (let attempt = 0; attempt < 5; attempt++) {
      if (candidates.length === 0) return [];

      const data = candidates.map((c) => ({
        value: c.size,
        node: c,
        percent: c.size / totalSize,
      }));

      const result = squarify(data, container) as Array<{
        x0: number; y0: number; x1: number; y1: number;
        value: number; node: DirNode; percent: number;
      }>;

      // Check if all cards meet minimum dimensions
      const tooSmall = new Set<string>();
      for (const r of result) {
        if (r.x1 - r.x0 < MIN_W || r.y1 - r.y0 < MIN_H) {
          tooSmall.add(r.node.path);
        }
      }

      if (tooSmall.size === 0) return result;

      // Remove too-small cards and re-layout
      candidates = candidates.filter((c) => !tooSmall.has(c.path));
    }

    return [];
  }, [node, dims]);

  if (node.children.length === 0) {
    return (
      <div className="treemap-container" ref={containerRef}>
        <div className="treemap-empty">
          <span>Empty folder</span>
        </div>
      </div>
    );
  }

  return (
    <div className="treemap-container" ref={containerRef}>
      {rects.map((r) => {
        const x = r.x0;
        const y = r.y0;
        const w = r.x1 - r.x0;
        const h = r.y1 - r.y0;
        const canDrill = r.node.is_dir && r.node.children.length > 0;
        const pct = Math.round(r.percent * 100);

        // Opacity: ranges from 0.04 (tiny) to 0.12 (dominant)
        const opacity = 0.04 + r.percent * 0.12;

        // Name always shows (cards below 80×36 already filtered out)
        const showSize = h > 52;
        // Show full "size · %" when wide enough, otherwise just "%"
        const sizeText = w > 140 ? `${formatSize(r.node.size)} · ${pct}%` : `${pct}%`;

        return (
          <div
            key={r.node.path}
            className={`treemap-rect ${canDrill ? "treemap-drillable" : ""}`}
            style={{
              left: x + GAP / 2,
              top: y + GAP / 2,
              width: w - GAP,
              height: h - GAP,
              background: `rgba(255, 255, 255, ${opacity})`,
            }}
            onClick={() => canDrill && onDrillIn(r.node)}
            title={`${r.node.name} — ${formatSize(r.node.size)} · ${pct}%`}
          >
            <span className="treemap-label">{r.node.name}</span>
            {showSize && (
              <span className="treemap-size">
                {sizeText}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}
