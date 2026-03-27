import { ChevronRight, type LucideIcon } from "lucide-react";
import { useNavigate } from "react-router-dom";
import type { ReactNode } from "react";

interface ModuleCardProps {
  title: string;
  description: string;
  icon: LucideIcon;
  route: string;
  meta?: string;
  stat?: string;
  statLabel?: string;
  children?: ReactNode;
  style?: React.CSSProperties;
}

export default function ModuleCard({
  title,
  description,
  icon: Icon,
  route,
  meta,
  stat,
  statLabel,
  children,
  style,
}: ModuleCardProps) {
  const navigate = useNavigate();

  return (
    <div
      onClick={() => navigate(route)}
      style={{
        background: "var(--bg-card)",
        border: "1px solid var(--border)",
        borderRadius: 10,
        padding: 16,
        cursor: "pointer",
        display: "flex",
        flexDirection: "column",
        transition: "background 0.12s, border-color 0.12s",
        overflow: "hidden",
        ...style,
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--bg-card-hover)";
        e.currentTarget.style.borderColor = "var(--border-hover)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "var(--bg-card)";
        e.currentTarget.style.borderColor = "var(--border)";
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 9,
          marginBottom: 4,
        }}
      >
        <div
          style={{
            width: 28,
            height: 28,
            borderRadius: 7,
            background: "rgba(255,255,255,0.05)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <Icon size={14} color="var(--text-secondary)" strokeWidth={1.7} />
        </div>
        <span
          style={{
            fontSize: 13,
            fontWeight: 600,
            color: "var(--text-primary)",
          }}
        >
          {title}
        </span>
      </div>

      <div
        style={{
          fontSize: 12,
          color: "var(--text-tertiary)",
          lineHeight: 1.45,
        }}
      >
        {description}
      </div>

      {children}

      {stat && (
        <div style={{ marginTop: "auto" }}>
          <div
            style={{
              fontSize: 28,
              fontWeight: 600,
              color: "var(--text-primary)",
              lineHeight: 1,
              marginTop: "auto",
            }}
          >
            {stat}
          </div>
          {statLabel && (
            <div
              style={{
                fontSize: 11,
                color: "var(--text-tertiary)",
                marginTop: 4,
              }}
            >
              {statLabel}
            </div>
          )}
        </div>
      )}

      {meta && !stat && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginTop: "auto",
          }}
        >
          <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
            {meta}
          </span>
          <ChevronRight size={15} color="var(--text-tertiary)" strokeWidth={1.5} />
        </div>
      )}
    </div>
  );
}
