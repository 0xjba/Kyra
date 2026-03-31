import { useState, useEffect, useRef } from "react";
import { checkFullDiskAccess } from "../lib/tauri";
import { invoke } from "@tauri-apps/api/core";

export default function FdaPrompt() {
  const [hasAccess, setHasAccess] = useState<boolean | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const retriesRef = useRef(0);
  const MAX_RETRIES = 30; // 60 seconds max

  // Initial check
  useEffect(() => {
    checkFullDiskAccess().then(setHasAccess).catch(() => setHasAccess(true));
  }, []);

  // Poll every 2s — auto-restart when FDA is granted, max 30 retries
  useEffect(() => {
    if (hasAccess !== false || dismissed) return;
    retriesRef.current = 0;

    pollRef.current = setInterval(() => {
      retriesRef.current += 1;
      if (retriesRef.current > MAX_RETRIES) {
        if (pollRef.current) clearInterval(pollRef.current);
        return;
      }
      checkFullDiskAccess()
        .then((result) => {
          if (result) {
            if (pollRef.current) clearInterval(pollRef.current);
            invoke("restart_app");
          }
        })
        .catch(() => {});
    }, 2000);

    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [hasAccess, dismissed]);

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
          background: "rgba(30, 30, 30, 0.85)",
          backdropFilter: "blur(20px) saturate(150%)",
          WebkitBackdropFilter: "blur(20px) saturate(150%)",
          border: "1px solid rgba(255,255,255,0.1)",
          borderRadius: 12,
          padding: "24px 28px",
          maxWidth: 420,
          width: "90%",
          boxShadow: "0 8px 32px rgba(0, 0, 0, 0.4)",
        }}
      >
        <div style={{ fontSize: "var(--font-xl)", fontWeight: "var(--weight-semibold)", color: "var(--text-primary)", marginBottom: 10 }}>
          Full Disk Access Required
        </div>
        <div style={{ fontSize: "var(--font-md)", color: "var(--text-secondary)", lineHeight: 1.6, marginBottom: 16 }}>
          Kyra needs Full Disk Access to scan and clean caches, logs, and browser data
          in protected directories.
        </div>
        <div style={{ fontSize: "var(--font-base)", color: "var(--text-tertiary)", lineHeight: 1.7, marginBottom: 16 }}>
          1. Click <strong style={{ color: "var(--text-secondary)" }}>Open Settings</strong> below<br />
          2. Click the <strong style={{ color: "var(--text-secondary)" }}>+</strong> button and add Kyra (or enable it if already listed)<br />
          3. Click <strong style={{ color: "var(--text-secondary)" }}>Restart Kyra</strong> below to apply changes
        </div>

        <div
          style={{
            fontSize: "var(--font-sm)",
            color: "var(--text-tertiary)",
            background: "rgba(255,255,255,0.03)",
            borderRadius: 6,
            padding: "8px 12px",
            marginBottom: 16,
            lineHeight: 1.6,
          }}
        >
          Without Full Disk Access, Kyra can only clean files in non-protected directories.
          Browser caches, mail data, and some system caches won't be accessible.
        </div>

        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="btn" onClick={() => setDismissed(true)}>
            Continue with Limited Access
          </button>
          <button className="btn" onClick={() => invoke("open_fda_settings")}>
            Open Settings
          </button>
          <button className="btn btn-primary" onClick={() => invoke("restart_app")}>
            Restart Kyra
          </button>
        </div>
      </div>
    </div>
  );
}
