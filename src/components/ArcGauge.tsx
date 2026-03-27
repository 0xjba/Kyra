import { useRef, useEffect } from "react";

interface ArcGaugeProps {
  value: number;
  max: number;
  label: string;
  detail: string;
  color: string;
  size?: number;
}

export default function ArcGauge({
  value,
  max,
  label,
  detail,
  color,
  size = 140,
}: ArcGaugeProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const percent = max > 0 ? Math.min(value / max, 1) : 0;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, size, size);

    const cx = size / 2;
    const cy = size / 2;
    const radius = size / 2 - 14;
    const lineWidth = 8;
    const startAngle = 0.75 * Math.PI;
    const endAngle = 2.25 * Math.PI;
    const sweep = endAngle - startAngle;

    // Background track
    ctx.beginPath();
    ctx.arc(cx, cy, radius, startAngle, endAngle);
    ctx.strokeStyle = "rgba(255, 255, 255, 0.06)";
    ctx.lineWidth = lineWidth;
    ctx.lineCap = "round";
    ctx.stroke();

    // Value arc
    if (percent > 0) {
      ctx.beginPath();
      ctx.arc(cx, cy, radius, startAngle, startAngle + sweep * percent);
      ctx.strokeStyle = color;
      ctx.lineWidth = lineWidth;
      ctx.lineCap = "round";
      ctx.stroke();
    }

    // Center percentage text
    ctx.fillStyle = "rgba(255, 255, 255, 0.88)";
    ctx.font = `600 ${size * 0.17}px -apple-system, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(`${Math.round(percent * 100)}%`, cx, cy - 4);

    // Label below percentage
    ctx.fillStyle = "rgba(255, 255, 255, 0.25)";
    ctx.font = `400 ${size * 0.08}px -apple-system, sans-serif`;
    ctx.fillText(label, cx, cy + size * 0.12);
  }, [value, max, label, color, size, percent]);

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
      <canvas
        ref={canvasRef}
        style={{ width: size, height: size }}
      />
      <div style={{ fontSize: 11, color: "rgba(255, 255, 255, 0.4)", textAlign: "center" }}>
        {detail}
      </div>
    </div>
  );
}
