import { useState, useEffect } from "react";
import { checkFullDiskAccess } from "../lib/tauri";

export default function FdaPrompt() {
  const [hasAccess, setHasAccess] = useState<boolean | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    checkFullDiskAccess().then(setHasAccess).catch(() => setHasAccess(true));
  }, []);

  if (hasAccess === null || hasAccess || dismissed) return null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0, 0, 0, 0.7)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
    >
      <div
        style={{
          background: "#1e1e1e",
          border: "1px solid rgba(255,255,255,0.1)",
          borderRadius: 12,
          padding: "24px 28px",
          maxWidth: 420,
          width: "90%",
        }}
      >
        <div style={{ fontSize: 15, fontWeight: 600, color: "rgba(255,255,255,0.88)", marginBottom: 10 }}>
          Full Disk Access Required
        </div>
        <div style={{ fontSize: 13, color: "rgba(255,255,255,0.5)", lineHeight: 1.6, marginBottom: 16 }}>
          Kyra needs Full Disk Access to scan and clean caches, logs, and browser data
          in protected directories.
        </div>
        <div style={{ fontSize: 12, color: "rgba(255,255,255,0.4)", lineHeight: 1.7, marginBottom: 20 }}>
          1. Open <strong style={{ color: "rgba(255,255,255,0.7)" }}>System Settings</strong><br />
          2. Go to <strong style={{ color: "rgba(255,255,255,0.7)" }}>Privacy & Security → Full Disk Access</strong><br />
          3. Enable <strong style={{ color: "rgba(255,255,255,0.7)" }}>Kyra</strong><br />
          4. Restart the app
        </div>
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button
            onClick={() => setDismissed(true)}
            style={{
              padding: "6px 16px",
              background: "rgba(255,255,255,0.06)",
              border: "1px solid rgba(255,255,255,0.1)",
              borderRadius: 6,
              color: "rgba(255,255,255,0.5)",
              fontSize: 12,
              cursor: "pointer",
            }}
          >
            Continue Anyway
          </button>
          <button
            onClick={() => {
              import("@tauri-apps/api/core").then(({ invoke }) => {
                invoke("reveal_in_finder", {
                  path: "/System/Library/PreferencePanes/Security.prefPane",
                });
              });
            }}
            style={{
              padding: "6px 16px",
              background: "rgba(255,255,255,0.1)",
              border: "1px solid rgba(255,255,255,0.15)",
              borderRadius: 6,
              color: "rgba(255,255,255,0.88)",
              fontSize: 12,
              cursor: "pointer",
            }}
          >
            Open Settings
          </button>
        </div>
      </div>
    </div>
  );
}
