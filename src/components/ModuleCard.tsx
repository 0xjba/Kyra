import { type LucideIcon } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { memo } from "react";

interface ModuleCardProps {
  title: string;
  icon: LucideIcon;
  route: string;
  value?: string;
  meta?: string;
  size?: "wide" | "big" | "small";
  tint?: "red" | "yellow" | "green";
  flag?: boolean;
  style?: React.CSSProperties;
}

function ModuleCard({
  title,
  icon: Icon,
  route,
  value,
  meta,
  size = "small",
  tint,
  flag,
  style,
}: ModuleCardProps) {
  const navigate = useNavigate();
  const tintClass = tint ? ` module-card-tinted-${tint}` : "";

  return (
    <div
      className={`module-card module-card-${size}${tintClass}`}
      onClick={() => navigate(route)}
      style={style}
    >
      <div className="module-card-header">
        <Icon size={15} color="currentColor" strokeWidth={1.7} />
        <span className="module-card-title">{title}</span>
        {flag && <span className="module-card-flag" />}
      </div>

      <div className="module-card-footer">
        {value && <div className="module-card-value">{value}</div>}
        {meta && <div className="module-card-meta-text">{meta}</div>}
      </div>
    </div>
  );
}

export default memo(ModuleCard);
