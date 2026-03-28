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
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setWidth(entry.contentRect.width);
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
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

    const maxVal = Math.max(
      ...history.map((p) => Math.max(p.upload, p.download)),
      1024
    );

    const padding = 2;
    const graphW = width - padding * 2;
    const graphH = height - padding * 2;

    function drawLine(data: number[], color: string) {
      if (!ctx) return;
      ctx.beginPath();
      for (let i = 0; i < data.length; i++) {
        const x = padding + (i / (data.length - 1)) * graphW;
        const y = padding + graphH - (data[i] / maxVal) * graphH;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }

    // Download line (cyan)
    drawLine(
      history.map((p) => p.download),
      "#5ef5e2"
    );

    // Upload line (green)
    drawLine(
      history.map((p) => p.upload),
      "#4ade80"
    );
  }, [history, width, height]);

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", marginBottom: 6 }}>
        <span style={{ fontSize: 12, color: "rgba(255, 255, 255, 0.5)" }}>Network</span>
        <div style={{ display: "flex", gap: 12, fontSize: 11 }}>
          <span style={{ color: "#5ef5e2" }}>
            {"\u2193"} {latest ? formatRate(latest.download) : "\u2014"}
          </span>
          <span style={{ color: "#4ade80" }}>
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
