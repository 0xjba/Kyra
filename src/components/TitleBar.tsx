import { useNavigate, useLocation } from "react-router-dom";
import { ArrowLeft } from "lucide-react";
import { useNavigationStore } from "../stores/navigationStore";

export default function TitleBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const isHome = location.pathname === "/";
  const backOverride = useNavigationStore((s) => s.backOverride);

  const handleBack = () => {
    if (backOverride) {
      backOverride();
    } else {
      navigate("/");
    }
  };

  return (
    <div
      data-tauri-drag-region
      style={{
        height: 28,
        flexShrink: 0,
        position: "relative",
        zIndex: 10,
      }}
    >
      {/* Decorative pill behind the native traffic lights */}
      <div
        data-tauri-drag-region
        style={{
          position: "absolute",
          left: 4,
          top: 5,
          width: 60,
          height: 19,
          borderRadius: 10,
          background: "rgba(255, 255, 255, 0.04)",
          border: "1px solid rgba(255, 255, 255, 0.06)",
          pointerEvents: "none",
        }}
      />

      {/* Back button — small circle right after traffic lights pill */}
      {!isHome && (
        <button
          onClick={handleBack}
          style={{
            position: "absolute",
            left: 68,
            top: 5,
            width: 19,
            height: 19,
            borderRadius: 10,
            background: "rgba(255, 255, 255, 0.06)",
            border: "1px solid rgba(255, 255, 255, 0.08)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--text-secondary)",
            padding: 0,
            transition: "background 0.12s",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "rgba(255, 255, 255, 0.12)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "rgba(255, 255, 255, 0.06)";
          }}
        >
          <ArrowLeft size={10} strokeWidth={2} />
        </button>
      )}

      {/* Centered title */}
      <div
        data-tauri-drag-region
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: "var(--font-md)",
          fontWeight: "var(--weight-semibold)",
          color: "var(--text-secondary)",
          pointerEvents: "none",
        }}
      >
        Kyra
      </div>
    </div>
  );
}
