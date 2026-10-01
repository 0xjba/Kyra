import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useCallback } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { ChevronLeft, ChevronRight, Settings, Siren, LockKeyhole, Check, AlertTriangle } from "lucide-react";
import { useNavigationStore } from "../stores/navigationStore";
import { useGuardianStore } from "../stores/guardianStore";
import { useSettingsStore } from "../stores/settingsStore";
import { formatSize } from "../utils/format";
import { patrolStatusLine } from "../utils/patrol";
import SubscribeSheet from "./SubscribeSheet";
import RestoreSheet from "./RestoreSheet";
import "../styles/guardian.css";

const TEASER = [
  { name: "Node.js", tier: "#2AC852" },
  { name: "Xcode", tier: "#2AC852" },
  { name: "System & Logs", tier: "#2AC852" },
  { name: "AI & ML Models", tier: "#FD4841" },
];

const PERKS = [
  "Runs daily and when space runs low",
  "Auto-cleans safe caches on its own",
  "Asks before touching your data",
];

const SCREEN_NAMES: Record<string, string> = {
  "/": "Kyra",
  "/clean": "Clean",
  "/status": "Status",
  "/analyze": "Analyze",
  "/uninstall": "Uninstall",
  "/settings": "Settings",
  "/prune": "Prune",
  "/installers": "Installers",
  "/optimize": "Optimize",
  "/guardian": "Pawtrol",
  "/onboarding": "",
};

export default function TitleBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const isHome = location.pathname === "/";
  const onboardingDone = useSettingsStore((s) => s.settings.onboarding_completed);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const isOnboarding = settingsLoaded && !onboardingDone;
  const backOverride = useNavigationStore((s) => s.backOverride);
  const [popOpen, setPopOpen] = useState(false);
  const [sheet, setSheet] = useState<"subscribe" | "restore" | null>(null);
  const closeSheet = useCallback(() => setSheet(null), []);
  const openSheet = (which: "subscribe" | "restore") => {
    setPopOpen(false);
    setSheet(which);
  };

  const title = isOnboarding ? "" : SCREEN_NAMES[location.pathname] ?? location.pathname.slice(1);

  const handleBack = () => {
    if (backOverride) backOverride();
    else navigate("/");
  };

  const togglePop = useCallback(() => setPopOpen((o) => !o), []);

  useEffect(() => {
    setPopOpen(false);
  }, [location.pathname]);

  const isPro = useGuardianStore((s) => s.license.active);
  const expires = useGuardianStore((s) => s.license.expires);
  const deviceName = useGuardianStore((s) => s.deviceName);
  const patrol = useGuardianStore((s) => s.patrolStatus);
  const checkLicense = useGuardianStore((s) => s.checkLicense);
  const loadPatrolStatus = useGuardianStore((s) => s.loadPatrolStatus);
  const [windowFocused, setWindowFocused] = useState(() => document.hasFocus());
  useEffect(() => {
    const onFocus = () => setWindowFocused(true);
    const onBlur = () => setWindowFocused(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  // Classic macOS geometry until the real values arrive from AppKit.
  const [lights, setLights] = useState({ right: 72, dot: 12, gap: 8 });

  useEffect(() => {
    invoke<{ right: number; dot: number; gap: number } | null>("get_traffic_lights")
      .then((l) => { if (l) setLights(l); })
      .catch(() => {});
  }, []);

  useEffect(() => {
    checkLicense();
  }, [checkLicense]);

  useEffect(() => {
    if (isPro) loadPatrolStatus();
  }, [isPro, loadPatrolStatus]);

  const pendingCount = patrol?.pending_review.length ?? 0;
  const freedBadge = patrol ? patrol.freed_last || patrol.freed_total : 0;
  let chipBadge = "PRO";
  let chipColor = "linear-gradient(90deg,#FD4841,#FD8C34 40%,#2AC852 75%,#22B8F0)";
  if (isPro) {
    if (pendingCount > 0) {
      chipBadge = String(pendingCount);
      chipColor = "#FDB022";
    } else if (patrol && !patrol.enabled) {
      chipBadge = "OFF";
      chipColor = "rgba(13,21,38,0.35)";
    } else {
      chipBadge = freedBadge > 0 ? formatSize(freedBadge) : "ON";
      chipColor = "#2AC852";
    }
  }

  const renewDate = expires
    ? new Date(expires * 1000).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" })
    : null;
  const popSub = isPro
    ? renewDate ? `Renews ${renewDate}` : "Active"
    : `Keeps your ${deviceName || "Mac"} clean on its own`;

  const openPawtrol = () => {
    setPopOpen(false);
    navigate("/guardian");
  };

  const statusDot = !patrol ? "rgba(13,21,38,0.3)" : patrol.running ? "#1f5fff" : patrol.enabled ? "#2AC852" : "#FDB022";

  return (
    <>
      <div
        data-tauri-drag-region
        style={{
          height: 54,
          flexShrink: 0,
          position: "relative",
          zIndex: 10,
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "0 16px 0 20px",
        }}
      >
        {/* Native traffic lights sit here; their width varies by macOS version */}
        <div style={{ width: lights.right - 20, flexShrink: 0, marginRight: lights.gap - 12 }} data-tauri-drag-region />

        {/* Back button */}
        {!isHome && !isOnboarding && (
          <button
            onClick={handleBack}
            className={`titlebar-btn-circle${windowFocused ? "" : " inactive"}`}
            style={{ width: lights.dot + 1, height: lights.dot + 1, borderRadius: (lights.dot + 1) / 2 }}
            aria-label="Back"
          >
            <ChevronLeft size={Math.round((lights.dot + 1) * 0.8)} strokeWidth={3.25} style={{ marginLeft: -1 }} />
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
            fontSize: 13,
            fontWeight: 600,
            color: "var(--text-secondary)",
            pointerEvents: "none",
          }}
        >
          {title}
        </div>

        {/* Right controls */}
        {!isOnboarding && (
          <div className="titlebar-right">
            <button
              className={`titlebar-chip${popOpen ? " open" : ""}`}
              onClick={togglePop}
            >
              <Siren size={14} style={{ color: isPro ? undefined : "var(--text-tertiary)" }} />
              <span>Pawtrol</span>
              <span
                className="titlebar-chip-badge"
                style={{
                  background: chipColor,
                  font: isPro
                    ? "700 11px/1 ui-monospace,'SF Mono',Menlo,monospace"
                    : "800 10px/1 -apple-system,system-ui",
                  letterSpacing: isPro ? "0" : "0.06em",
                }}
              >
                {chipBadge}
              </span>
            </button>
            <button className="titlebar-gear" onClick={() => navigate("/settings")}>
              <Settings size={15} />
            </button>
          </div>
        )}
      </div>

      {/* Pawtrol Popover */}
      {popOpen && (
        <>
          <div className="paw-pop-backdrop" onClick={togglePop} />
          <div className="paw-pop">
            {/* Header */}
            <div className="paw-pop-header">
              <div className="paw-pop-icon">
                <Siren size={20} style={{ color: isPro ? undefined : "var(--text-tertiary)" }} />
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <span style={{ fontSize: 15, fontWeight: 700, letterSpacing: "-0.01em" }}>Pawtrol</span>
                  {isPro ? (
                    <span className="paw-pop-active-pill">Pro · Active</span>
                  ) : (
                    <span className="paw-pop-pro-pill">PRO</span>
                  )}
                </div>
                <div style={{ fontSize: 12, color: "var(--text-secondary)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {popSub}
                </div>
              </div>
            </div>

            {isPro && (
              <>
                <div className="paw-pop-summary">
                  <div className="paw-pop-summary-head">
                    <span className="paw-pop-summary-size">{formatSize(patrol?.freed_total ?? 0)}</span>
                    <span className="paw-pop-summary-label">freed by Pawtrol</span>
                  </div>
                  <div className="paw-pop-summary-meta guardian-pop-status">
                    <span className="guardian-duty-dot" style={{ background: statusDot, boxShadow: "none" }} />
                    <span>{patrol ? patrolStatusLine(patrol) : "Checking in…"}</span>
                  </div>
                </div>
                {pendingCount > 0 ? (
                  <button className="guardian-pop-review" onClick={openPawtrol}>
                    <AlertTriangle size={13} strokeWidth={2.2} />
                    <span>{pendingCount} need{pendingCount === 1 ? "s" : ""} your review</span>
                    <ChevronRight size={14} />
                  </button>
                ) : (
                  <div className="guardian-pop-quiet">Nothing needs your review.</div>
                )}
                <div className="paw-pop-actions">
                  <button
                    className="btn btn-primary"
                    style={{ flex: 1, height: 34, borderRadius: 999 }}
                    onClick={openPawtrol}
                  >
                    Open Pawtrol
                  </button>
                </div>
              </>
            )}

            {/* Free user content */}
            {!isPro && (
              <>
                {/* Blurred teaser */}
                <div className="paw-pop-teaser">
                  <div className="paw-pop-teaser-blur" aria-hidden="true">
                    {TEASER.map((c) => (
                      <div key={c.name} className="paw-pop-cat-row">
                        <span className="paw-pop-cat-name">{c.name}</span>
                        <span className="paw-pop-cat-score paw-pop-teaser-pill" style={{ background: c.tier }} />
                        <span className="paw-pop-teaser-size" />
                      </div>
                    ))}
                  </div>
                  <div className="paw-pop-teaser-lock">
                    <LockKeyhole size={13} />
                    What Pawtrol finds shows up here
                  </div>
                </div>

                {/* Perks */}
                <div className="paw-pop-perks">
                  {PERKS.map((k) => (
                    <div key={k} className="paw-pop-perk">
                      <span className="paw-pop-perk-check"><Check size={10} strokeWidth={3} /></span>
                      {k}
                    </div>
                  ))}
                </div>

                {/* Pricing + Subscribe */}
                <div className="paw-pop-pricing">
                  <div style={{ display: "flex", alignItems: "baseline", gap: 2 }}>
                    <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>from</span>
                    <span style={{ fontSize: 20, fontWeight: 700, letterSpacing: "-0.03em" }}>$0.99</span>
                    <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>/month</span>
                  </div>
                  <button
                    className="btn btn-primary"
                    style={{ marginLeft: "auto", height: 34, padding: "0 18px", borderRadius: 999 }}
                    onClick={() => openSheet("subscribe")}
                  >
                    Subscribe
                  </button>
                </div>
                <button className="paw-pop-refresh" onClick={() => openSheet("restore")}>
                  Already subscribed? Restore
                </button>
              </>
            )}
          </div>
        </>
      )}
      <SubscribeSheet open={sheet === "subscribe"} onClose={closeSheet} />
      <RestoreSheet open={sheet === "restore"} onClose={closeSheet} />
    </>
  );
}
