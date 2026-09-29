import { useMemo, useRef, useEffect, useState } from "react";
import squarify from "squarify";
import type { DirNode } from "../lib/tauri";
import { formatSize } from "../utils/format";

export interface TreemapItem {
  node: DirNode;
  fill: string;
}

interface TreemapProps {
  items: TreemapItem[];
  animKey: string;
  hoveredPath: string | null;
  onDrillIn: (node: DirNode) => void;
  onHover?: (node: DirNode) => void;
}

const MAX_CELLS = 40;
const MIN_SHARE = 0.004;

export default function Treemap({ items, animKey, hoveredPath, onDrillIn, onHover }: TreemapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [dims, setDims] = useState({ w: 520, h: 460 });
  const [shown, setShown] = useState(true);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let rafId: number;
    const observer = new ResizeObserver((entries) => {
      cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        for (const entry of entries) {
          setDims({ w: entry.contentRect.width, h: entry.contentRect.height });
        }
      });
    });
    observer.observe(el);
    return () => {
      cancelAnimationFrame(rafId);
      observer.disconnect();
    };
  }, []);

  useEffect(() => {
    setShown(false);
    const t = setTimeout(() => setShown(true), 30);
    return () => clearTimeout(t);
  }, [animKey]);

  const rects = useMemo(() => {
    if (dims.w < 10 || dims.h < 10) return [];
    const nonEmpty = items.filter((it) => it.node.size > 0);
    const total = nonEmpty.reduce((a, it) => a + it.node.size, 0);
    if (total <= 0) return [];
    const visible = nonEmpty
      .filter((it, i) => i === 0 || it.node.size / total >= MIN_SHARE)
      .slice(0, MAX_CELLS);
    const data = visible.map((it) => ({ value: it.node.size, item: it }));
    return squarify(data, { x0: 0, y0: 0, x1: dims.w, y1: dims.h }) as Array<{
      x0: number; y0: number; x1: number; y1: number; value: number; item: TreemapItem;
    }>;
  }, [items, dims]);

  return (
    <div
      className="analyze-map"
      ref={containerRef}
      style={{ opacity: shown ? 1 : 0, transform: shown ? "none" : "scale(0.97)" }}
    >
      {items.length === 0 && <div className="analyze-map-empty">This folder is empty</div>}
      {rects.map((r) => {
        const w = r.x1 - r.x0;
        const h = r.y1 - r.y0;
        const { node, fill } = r.item;
        const canDrill = node.is_dir && node.children.length > 0;
        const big = w > 90 && h > 54;
        const hovered = hoveredPath === node.path;
        return (
          <div
            key={node.path}
            className={`analyze-cell${canDrill ? " drillable" : ""}`}
            style={{ left: r.x0, top: r.y0, width: w, height: h }}
            onClick={() => canDrill && onDrillIn(node)}
            onMouseEnter={() => onHover?.(node)}
            title={`${node.name} — ${formatSize(node.size)}`}
          >
            <div
              className={`analyze-cell-inner${hovered ? " hovered" : ""}`}
              style={{
                borderRadius: Math.min(w, h) > 80 ? 16 : 10,
                background: `linear-gradient(160deg, rgba(255,255,255,0.4), rgba(255,255,255,0) 55%), ${fill}`,
              }}
            >
              {big && (
                <>
                  <div className="analyze-cell-name" style={{ fontSize: w > 200 && h > 120 ? 16 : 13 }}>
                    {node.name}
                  </div>
                  <div className="analyze-cell-size">{formatSize(node.size)}</div>
                </>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
