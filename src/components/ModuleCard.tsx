import { ChevronRight, type LucideIcon } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { memo, type ReactNode } from "react";

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

function ModuleCard({
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
      className="module-card"
      onClick={() => navigate(route)}
      style={style}
    >
      <div className="module-card-header">
        <div className="module-card-icon">
          <Icon size={13} color="var(--text-secondary)" strokeWidth={1.7} />
        </div>
        <span className="module-card-title">{title}</span>
      </div>

      {description && (
        <div className="module-card-desc">{description}</div>
      )}

      {children}

      {stat && (
        <div className="module-card-stat">
          <div className="module-card-stat-value">{stat}</div>
          {statLabel && (
            <div className="module-card-stat-label">{statLabel}</div>
          )}
        </div>
      )}

      {meta && !stat && (
        <div className="module-card-meta">
          <span className="module-card-meta-text">{meta}</span>
          <ChevronRight size={13} color="var(--text-tertiary)" strokeWidth={1.5} />
        </div>
      )}
    </div>
  );
}

export default memo(ModuleCard);
