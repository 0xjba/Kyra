import { useRef, useEffect, useState } from "react";

interface NetworkPoint {
  upload: number;
  download: number;
}

interface NetworkGraphProps {
  history: NetworkPoint[];
  height?: number;
}

function formatRate(bytesPerSec: number): string {
  if (bytesPerSec >= 1024 * 1024) {
    return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
  }
  if (bytesPerSec >= 1024) {
    return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  }
  return `${bytesPerSec} B/s`;
}

export default function NetworkGraph({ history, height = 120 }: NetworkGraphProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(460);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let rafId: number;
    const observer = new ResizeObserver((entries) => {
      cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        for (const entry of entries) {
          setWidth(entry.contentRect.width);
        }
      });
    });
    observer.observe(container);
    return () => { cancelAnimationFrame(rafId); observer.disconnect(); };
  }, []);

  const latest = history.length > 0 ? history[history.length - 1] : null;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, height);

    if (history.length < 2) return;

    const rawMax = history.reduce(
      (acc, p) => Math.max(acc, isFinite(p.upload) ? p.upload : 0, isFinite(p.download) ? p.download : 0),
      0
    );
    const maxVal = Math.max(rawMax, 1024);

    const padding = 2;
    const graphW = width - padding * 2;
    const graphH = height - padding * 2;

    function drawLine(data: number[], color: string) {
      if (!ctx || data.length < 2) return;
      ctx.beginPath();
      let started = false;
      for (let i = 0; i < data.length; i++) {
        const val = isFinite(data[i]) ? data[i] : 0;
        const x = padding + (i / (data.length - 1)) * graphW;
        const y = padding + graphH - (val / maxVal) * graphH;
        if (!started) {
          ctx.moveTo(x, y);
          started = true;
        } else {
          ctx.lineTo(x, y);
        }
      }
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }

    // Download line (cyan)
    drawLine(
      history.map((p) => p.download),
      "#22B8F0"
    );

    // Upload line (green)
    drawLine(
      history.map((p) => p.upload),
      "#2AC852"
    );
  }, [history, width, height]);

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", marginBottom: 6 }}>
        <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>Network</span>
        <div style={{ display: "flex", gap: 12, fontSize: 11 }}>
          <span style={{ color: "#22B8F0" }}>
            {"\u2193"} {latest ? formatRate(latest.download) : "\u2014"}
          </span>
          <span style={{ color: "#2AC852" }}>
            {"\u2191"} {latest ? formatRate(latest.upload) : "\u2014"}
          </span>
        </div>
      </div>
      <div
        ref={containerRef}
        style={{
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          borderRadius: 8,
          overflow: "hidden",
        }}
      >
        <canvas
          ref={canvasRef}
          style={{ width, height, display: "block" }}
        />
      </div>
    </div>
  );
}
