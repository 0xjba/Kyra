import { useRef, useEffect, useCallback } from "react";
import type { DirNode } from "../lib/tauri";

interface SunburstProps {
  node: DirNode;
  onDrillIn: (node: DirNode) => void;
  onReveal: (path: string) => void;
  width: number;
  height: number;
}

interface ArcSegment {
  node: DirNode;
  depth: number;
  startAngle: number;
  endAngle: number;
}

const MAX_RINGS = 4;
const CENTER_RADIUS = 50;
const RING_WIDTH = 40;

const RING_COLORS = [
  "rgba(255, 255, 255, 0.12)",
  "rgba(255, 255, 255, 0.09)",
  "rgba(255, 255, 255, 0.07)",
  "rgba(255, 255, 255, 0.05)",
];

const HOVER_COLOR = "rgba(255, 255, 255, 0.20)";

function buildSegments(node: DirNode): ArcSegment[] {
  const segments: ArcSegment[] = [];

  function walk(n: DirNode, depth: number, startAngle: number, sweep: number) {
    if (depth > MAX_RINGS || sweep < 0.01) return;

    segments.push({
      node: n,
      depth,
      startAngle,
      endAngle: startAngle + sweep,
    });

    if (!n.is_dir || n.children.length === 0 || n.size === 0) return;

    let angle = startAngle;
    for (const child of n.children) {
      if (child.size === 0) continue;
      const childSweep = (child.size / n.size) * sweep;
      walk(child, depth + 1, angle, childSweep);
      angle += childSweep;
    }
  }

  if (node.children.length > 0 && node.size > 0) {
    let angle = 0;
    for (const child of node.children) {
      if (child.size === 0) continue;
      const sweep = (child.size / node.size) * Math.PI * 2;
      walk(child, 1, angle, sweep);
      angle += sweep;
    }
  }

  return segments;
}

function hitTest(
  segments: ArcSegment[],
  mx: number,
  my: number,
  cx: number,
  cy: number
): ArcSegment | null {
  const dx = mx - cx;
  const dy = my - cy;
  const dist = Math.sqrt(dx * dx + dy * dy);
  let angle = Math.atan2(dy, dx);
  if (angle < 0) angle += Math.PI * 2;

  for (let i = segments.length - 1; i >= 0; i--) {
    const seg = segments[i];
    const innerR = CENTER_RADIUS + (seg.depth - 1) * RING_WIDTH;
    const outerR = CENTER_RADIUS + seg.depth * RING_WIDTH;
    if (dist >= innerR && dist <= outerR) {
      if (angle >= seg.startAngle && angle < seg.endAngle) {
        return seg;
      }
    }
  }
  return null;
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

export default function Sunburst({ node, onDrillIn, onReveal, width, height }: SunburstProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const segmentsRef = useRef<ArcSegment[]>([]);
  const hoveredRef = useRef<ArcSegment | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);

  const cx = width / 2;
  const cy = height / 2;

  const draw = useCallback(
    (hovered: ArcSegment | null) => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;

      const dpr = window.devicePixelRatio || 1;
      canvas.width = width * dpr;
      canvas.height = height * dpr;
      ctx.scale(dpr, dpr);

      ctx.clearRect(0, 0, width, height);

      // Draw center circle
      ctx.beginPath();
      ctx.arc(cx, cy, CENTER_RADIUS, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(255, 255, 255, 0.06)";
      ctx.fill();

      // Draw center text
      ctx.fillStyle = "rgba(255, 255, 255, 0.7)";
      ctx.font = "12px -apple-system, sans-serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(formatSize(node.size), cx, cy - 6);
      ctx.fillStyle = "rgba(255, 255, 255, 0.4)";
      ctx.font = "10px -apple-system, sans-serif";
      ctx.fillText(node.name || "/", cx, cy + 8);

      // Draw arcs
      for (const seg of segmentsRef.current) {
        const innerR = CENTER_RADIUS + (seg.depth - 1) * RING_WIDTH;
        const outerR = CENTER_RADIUS + seg.depth * RING_WIDTH;
        const isHovered = hovered === seg;

        ctx.beginPath();
        ctx.arc(cx, cy, outerR, seg.startAngle, seg.endAngle);
        ctx.arc(cx, cy, innerR, seg.endAngle, seg.startAngle, true);
        ctx.closePath();

        ctx.fillStyle = isHovered
          ? HOVER_COLOR
          : RING_COLORS[Math.min(seg.depth - 1, RING_COLORS.length - 1)];
        ctx.fill();

        ctx.strokeStyle = "#191919";
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    },
    [node, width, height, cx, cy]
  );

  useEffect(() => {
    segmentsRef.current = buildSegments(node);
    draw(null);
  }, [node, draw]);

  const handleMouseMove = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;

      const hit = hitTest(segmentsRef.current, mx, my, cx, cy);
      if (hit !== hoveredRef.current) {
        hoveredRef.current = hit;
        draw(hit);
      }

      const tooltip = tooltipRef.current;
      if (tooltip) {
        if (hit) {
          tooltip.style.display = "block";
          tooltip.style.left = `${e.clientX - rect.left + 12}px`;
          tooltip.style.top = `${e.clientY - rect.top - 20}px`;
          tooltip.textContent = `${hit.node.name} — ${formatSize(hit.node.size)}`;
        } else {
          tooltip.style.display = "none";
        }
      }
    },
    [cx, cy, draw]
  );

  const handleClick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;

      const dx = mx - cx;
      const dy = my - cy;
      if (Math.sqrt(dx * dx + dy * dy) <= CENTER_RADIUS) {
        return;
      }

      const hit = hitTest(segmentsRef.current, mx, my, cx, cy);
      if (hit && hit.node.is_dir && hit.node.children.length > 0) {
        onDrillIn(hit.node);
      }
    },
    [cx, cy, onDrillIn]
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      e.preventDefault();
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;

      const hit = hitTest(segmentsRef.current, mx, my, cx, cy);
      if (hit) {
        onReveal(hit.node.path);
      }
    },
    [cx, cy, onReveal]
  );

  const handleMouseLeave = useCallback(() => {
    hoveredRef.current = null;
    draw(null);
    if (tooltipRef.current) tooltipRef.current.style.display = "none";
  }, [draw]);

  return (
    <div style={{ position: "relative", width, height }}>
      <canvas
        ref={canvasRef}
        onMouseMove={handleMouseMove}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
        onMouseLeave={handleMouseLeave}
        style={{ cursor: "pointer", width, height }}
      />
      <div
        ref={tooltipRef}
        style={{
          display: "none",
          position: "absolute",
          background: "rgba(0, 0, 0, 0.85)",
          color: "var(--text-primary)",
          fontSize: 11,
          padding: "4px 8px",
          borderRadius: 4,
          pointerEvents: "none",
          whiteSpace: "nowrap",
          zIndex: 10,
        }}
      />
    </div>
  );
}
