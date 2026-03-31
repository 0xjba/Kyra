import { useParams } from "react-router-dom";

const MODULE_NAMES: Record<string, string> = {
  clean: "Clean",
  optimize: "Optimize",
  uninstall: "Uninstall",
  analyze: "Analyze",
  status: "Status",
  purge: "Purge",
  installers: "Installers",
};

export default function ModulePlaceholder() {
  const { module } = useParams<{ module: string }>();
  const name = MODULE_NAMES[module || ""] || module;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        flexDirection: "column",
        gap: 8,
      }}
    >
      <div
        style={{
          fontSize: 15,
          fontWeight: 600,
          color: "var(--text-primary)",
        }}
      >
        {name}
      </div>
      <div style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
        Coming soon
      </div>
    </div>
  );
}
