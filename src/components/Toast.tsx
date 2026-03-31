import { useEffect, useState } from "react";

interface ToastProps {
  message: string;
  visible: boolean;
  onDone?: () => void;
  duration?: number;
  variant?: "success" | "error";
}

export default function Toast({
  message,
  visible,
  onDone,
  duration = 3000,
  variant = "success",
}: ToastProps) {
  const [show, setShow] = useState(false);

  useEffect(() => {
    if (visible) {
      setShow(true);
      const timer = setTimeout(() => {
        setShow(false);
        onDone?.();
      }, duration);
      return () => clearTimeout(timer);
    } else {
      setShow(false);
    }
  }, [visible, duration, onDone]);

  if (!show) return null;

  const isError = variant === "error";
  const accent = isError ? "#FD4841" : "#2AC852";

  return (
    <div
      style={{
        position: "fixed",
        bottom: 16,
        left: "50%",
        transform: "translateX(-50%)",
        zIndex: 100,
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "10px 18px",
        background: isError
          ? "rgba(253, 72, 65, 0.12)"
          : "rgba(42, 200, 82, 0.12)",
        border: `1px solid ${isError ? "rgba(253, 72, 65, 0.2)" : "rgba(42, 200, 82, 0.2)"}`,
        borderRadius: 8,
        backdropFilter: "blur(12px)",
        boxShadow: "0 4px 16px rgba(0, 0, 0, 0.3)",
        fontSize: 13,
        fontWeight: 500,
        color: "var(--text-primary)",
        animation: "toast-slide-in 0.2s ease",
      }}
    >
      <span style={{ color: accent, fontWeight: 600 }}>
        {isError ? "✕" : "✓"}
      </span>
      {message}
    </div>
  );
}
