import { useNavigate, useLocation } from "react-router-dom";
import { ArrowLeft } from "lucide-react";

export default function TitleBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const isHome = location.pathname === "/";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        padding: "14px 18px",
        borderBottom: "1px solid var(--border)",
      }}
    >
      <div style={{ width: 52, display: "flex", alignItems: "center" }}>
        {!isHome && (
          <button
            onClick={() => navigate("/")}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              display: "flex",
              alignItems: "center",
              color: "var(--text-secondary)",
              padding: 4,
              borderRadius: 4,
            }}
          >
            <ArrowLeft size={16} />
          </button>
        )}
      </div>
      <div
        style={{
          flex: 1,
          textAlign: "center",
          fontSize: 13,
          fontWeight: 600,
          color: "var(--text-secondary)",
        }}
      >
        Kyra
      </div>
      <div style={{ width: 52 }} />
    </div>
  );
}
