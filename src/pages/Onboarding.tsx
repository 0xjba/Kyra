import { useState, useEffect, useRef, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import {
  Trash2,
  Package,
  Download,
  Grid2x2Plus,
  HardDrive,
  Zap,
  Activity,
  Check,
  Siren,
} from "lucide-react";
import { useSettingsStore } from "../stores/settingsStore";
import { useGuardianStore } from "../stores/guardianStore";
import { checkFullDiskAccess, getDeviceName } from "../lib/tauri";
import { enable, disable } from "@tauri-apps/plugin-autostart";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import walk1 from "../assets/cat-walking/cat_walking_01.png";
import walk2 from "../assets/cat-walking/cat_walking_02.png";
import walk3 from "../assets/cat-walking/cat_walking_03.png";
import walk4 from "../assets/cat-walking/cat_walking_04.png";
import walk5 from "../assets/cat-walking/cat_walking_05.png";
import DemoPlayer from "../components/DemoPlayer";
import SubscribeSheet from "../components/SubscribeSheet";
import RestoreSheet from "../components/RestoreSheet";
import FdaMockup from "../components/FdaMockup";
import "../styles/onboarding.css";
import { skipFdaPromptThisLaunch } from "../components/FdaPrompt";

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];
const WALK_FRAMES = [walk1, walk2, walk3, walk4, walk5, walk4, walk3, walk2];

const STEPS = 6;

const FREE_MODULES = [
  { icon: Trash2, name: "Clean", desc: "Caches, logs, temp files" },
  { icon: Package, name: "Prune", desc: "node_modules, dist, target" },
  { icon: Download, name: "Installers", desc: "Find stale .dmg & .pkg" },
  { icon: Grid2x2Plus, name: "Uninstall", desc: "Remove apps completely" },
  { icon: HardDrive, name: "Analyze", desc: "Explore disk usage" },
  { icon: Zap, name: "Optimize", desc: "Repair caches & configs" },
  { icon: Activity, name: "Status", desc: "Live system monitor" },
];

const PERKS = ["Runs daily on its own", "Auto-cleans safe caches", "Asks before your data"];

type PrefKey = "login" | "trash" | "updates" | "notif";

const PREFS: [PrefKey, string, string][] = [
  ["login", "Launch at login", "Start Kyra when you log in"],
  ["trash", "Move to Trash", "Send files to Trash instead of deleting"],
  ["updates", "Check for updates", "Automatically on launch"],
  ["notif", "Notifications", "Low disk space and update alerts"],
];

export default function Onboarding() {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [direction, setDirection] = useState<"forward" | "backward">("forward");
  const [fdaGranted, setFdaGranted] = useState(false);
  const [macModel, setMacModel] = useState("Mac");
  const [sheet, setSheet] = useState<"subscribe" | "restore" | null>(null);
  const [completing, setCompleting] = useState(false);
  const current = useSettingsStore.getState().settings;
  const [prefs, setPrefs] = useState<Record<PrefKey, boolean>>(() => ({
    login: current.launch_at_login,
    trash: current.use_trash,
    updates: current.check_for_updates,
    notif: current.notifications_enabled,
  }));

  const [frame, setFrame] = useState(0);
  const [walk, setWalk] = useState(0);
  const [walkX, setWalkX] = useState(0);

  const setOnboardingCompleted = useSettingsStore((s) => s.setOnboardingCompleted);
  const setLaunchAtLogin = useSettingsStore((s) => s.setLaunchAtLogin);
  const setCheckForUpdates = useSettingsStore((s) => s.setCheckForUpdates);
  const setUseTrash = useSettingsStore((s) => s.setUseTrash);
  const setNotificationsEnabled = useSettingsStore((s) => s.setNotificationsEnabled);
  const guardianCheckLicense = useGuardianStore((s) => s.checkLicense);
  const pro = useGuardianStore((s) => s.license.active);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    getDeviceName().then(setMacModel).catch(() => {});
    guardianCheckLicense();
    checkFullDiskAccess().then(setFdaGranted).catch(() => {});
  }, [guardianCheckLicense]);

  useEffect(() => {
    if (step !== 0 && step !== 4) return;
    const id = setInterval(() => setFrame((f) => (f + 1) % CAT_FRAMES.length), 120);
    return () => clearInterval(id);
  }, [step]);

  useEffect(() => {
    if (step !== 5) return;
    const id = setInterval(() => {
      setWalk((w) => (w + 1) % WALK_FRAMES.length);
      setWalkX((x) => (x + 0.9) % 120);
    }, 120);
    return () => clearInterval(id);
  }, [step]);

  useEffect(() => {
    if (step !== 4) return;
    const onFocus = () => guardianCheckLicense();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [step, guardianCheckLicense]);

  const stopFdaPoll = () => {
    if (pollRef.current) clearInterval(pollRef.current);
    pollRef.current = null;
  };

  const startFdaPoll = useCallback(() => {
    if (pollRef.current) return;
    checkFullDiskAccess()
      .then((ok) => {
        setFdaGranted(ok);
        if (ok || pollRef.current) return;
        let retries = 0;
        pollRef.current = setInterval(async () => {
          retries++;
          const granted = await checkFullDiskAccess().catch(() => false);
          if (granted) {
            setFdaGranted(true);
            stopFdaPoll();
          } else if (retries >= 60) {
            stopFdaPoll();
          }
        }, 2000);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (step === 2) startFdaPoll();
    else stopFdaPoll();
  }, [step, startFdaPoll]);

  useEffect(() => {
    return () => {
      stopFdaPoll();
    };
  }, []);

  const goNext = () => {
    setDirection("forward");
    setStep((s) => Math.min(s + 1, STEPS - 1));
  };

  const goBack = () => {
    setDirection("backward");
    setStep((s) => Math.max(s - 1, 0));
  };

  const handleComplete = async () => {
    if (completing) return;
    setCompleting(true);
    if (!fdaGranted) skipFdaPromptThisLaunch();
    try {
      await setUseTrash(prefs.trash);
      await setCheckForUpdates(prefs.updates);
      await setNotificationsEnabled(prefs.notif);
      try {
        if (prefs.login) await enable();
        else await disable();
      } catch {}
      await setLaunchAtLogin(prefs.login);
      await setOnboardingCompleted(true);
      navigate("/");
    } finally {
      setCompleting(false);
    }
  };

  const openFdaSettings = async () => {
    if (fdaGranted) return;
    const { invoke } = await import("@tauri-apps/api/core");
    invoke("open_fda_settings").catch(() => {});
    startFdaPoll();
  };

  const closeSheet = useCallback(() => setSheet(null), []);

  const handleSubscribe = () => {
    if (!pro) setSheet("subscribe");
  };

  const togglePref = (key: PrefKey) => setPrefs((p) => ({ ...p, [key]: !p[key] }));

  const catSrc = CAT_FRAMES[frame];

  const renderWelcome = () => (
    <>
      <img src={catSrc} alt="" className="onboarding-logo" />
      <div className="onboarding-title">Kyra</div>
      <div className="onboarding-tagline">Nine lives for your storage</div>
      <div className="onboarding-rainbow-bar" />
    </>
  );

  const renderDemo = () => (
    <>
      <div className="onboarding-heading">See Kyra in action</div>
      <div className="onboarding-desc">Watch her free up space in seconds.</div>
      <DemoPlayer />
    </>
  );

  const renderFda = () => (
    <>
      <div className="onboarding-heading">Full Disk Access</div>
      <div className="onboarding-desc onboarding-desc-wrap">
        To reach protected spots like browser caches and system logs, Kyra needs Full Disk Access.
      </div>
      <FdaMockup granted={fdaGranted} />
      <button
        type="button"
        className={`onboarding-grant-btn${fdaGranted ? " granted" : ""}`}
        onClick={openFdaSettings}
      >
        {fdaGranted ? "Access granted ✓" : "Open System Settings"}
      </button>
    </>
  );

  const renderSetup = () => (
    <>
      <div className="onboarding-heading">Quick setup</div>
      <div className="onboarding-desc">You can change these any time in Settings.</div>
      <div className="onboarding-prefs-list">
        {PREFS.map(([key, name, desc]) => (
          <button
            type="button"
            key={key}
            role="switch"
            aria-checked={prefs[key]}
            className="onboarding-pref-item"
            onClick={() => togglePref(key)}
          >
            <div className="onboarding-pref-info">
              <div className="onboarding-pref-name">{name}</div>
              <div className="onboarding-pref-desc">{desc}</div>
            </div>
            <div className={`onboarding-toggle ${prefs[key] ? "on" : "off"}`}>
              <span className="onboarding-toggle-knob" />
            </div>
          </button>
        ))}
      </div>
    </>
  );

  const renderPro = () => (
    <>
      <div className="onboarding-heading">What's included</div>
      <div className="onboarding-desc">Everything here is free, forever.</div>
      <div className="onboarding-free-grid">
        {FREE_MODULES.map((mod) => (
          <div key={mod.name} className="onboarding-free-item">
            <mod.icon size={14} strokeWidth={2} className="onboarding-free-item-icon" />
            <span className="onboarding-free-item-name">{mod.name}</span>
            <span className="onboarding-free-item-desc">{mod.desc}</span>
          </div>
        ))}
        <div className="onboarding-feature-placeholder">Free · no account</div>
      </div>
      <div className="onboarding-pro-card">
        <img src={catSrc} alt="" className="onboarding-pro-cat" />
        <div className="onboarding-pro-body">
          <div className="onboarding-pro-title">
            <span className="onboarding-pro-name">Pawtrol</span>
            <span className="onboarding-pro-badge">PRO</span>
            <span className="onboarding-pro-price">from $0.99/mo</span>
          </div>
          <div className="onboarding-pro-pitch">
            Watches over your {macModel} daily and clears what's safe on its own.
          </div>
          <div className="onboarding-pro-perks">
            {PERKS.map((p) => (
              <span key={p} className="onboarding-pro-perk">
                <Check size={10} strokeWidth={3} />
                {p}
              </span>
            ))}
          </div>
          {!pro && (
            <button type="button" className="onboarding-pro-restore" onClick={() => setSheet("restore")}>
              Already subscribed? Restore
            </button>
          )}
        </div>
        <button
          type="button"
          className={`onboarding-subscribe-btn${pro ? " unlocked" : ""}`}
          onClick={handleSubscribe}
        >
          {pro ? "Pro unlocked ✓" : "Subscribe"}
        </button>
      </div>
    </>
  );

  const renderDone = () => (
    <>
      <div className="onboarding-walk-cat">
        <img src={WALK_FRAMES[walk]} alt="" style={{ left: `${walkX - 10}%` }} />
      </div>
      <div className="onboarding-heading onboarding-heading-lg">Kyra's on Pawtrol</div>
      <div className="onboarding-desc onboarding-done-desc">
        She grooms your caches clean. Hunts down stale builds. Chases out forgotten apps and keeps one
        eye on your disk. Purr-manently.
      </div>
      <div className="onboarding-done-chip">
        <svg width="0" height="0" style={{ position: "absolute" }} aria-hidden="true">
          <defs>
            <linearGradient id="onboarding-siren-grad" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="24" y2="0">
              <stop offset="50%" stopColor="#FD4841" />
              <stop offset="50%" stopColor="#1f5fff" />
            </linearGradient>
          </defs>
        </svg>
        <Siren size={13} strokeWidth={2} stroke="url(#onboarding-siren-grad)" />
        {pro
          ? "Pawtrol Pro is on. Run your first AI scan from the title bar."
          : "Pawtrol Pro is there whenever you want it. Everything else is free."}
      </div>
    </>
  );

  const screens = [renderWelcome, renderDemo, renderFda, renderSetup, renderPro, renderDone];
  const nextLabel = [
    "Get started",
    "Next",
    fdaGranted ? "Next" : "Skip for now",
    "Next",
    pro ? "Next" : "Maybe later",
    "Open Kyra",
  ][step];

  return (
    <div className="onboarding-container">
      <div className="onboarding-content">
        <div className={`onboarding-slide ${direction}`} key={step}>
          {screens[step]()}
        </div>
      </div>

      <div className="onboarding-footer">
        <div className="onboarding-dots">
          {Array.from({ length: STEPS }).map((_, i) => (
            <span key={i} className={`onboarding-dot${i === step ? " active" : ""}`} />
          ))}
        </div>
        <div className="onboarding-nav">
          {step > 0 && step < STEPS - 1 && (
            <button type="button" className="onboarding-back-btn" onClick={goBack}>
              Back
            </button>
          )}
          <button
            type="button"
            className="onboarding-next-btn"
            onClick={step === STEPS - 1 ? handleComplete : goNext}
            disabled={completing}
          >
            {nextLabel}
          </button>
        </div>
      </div>

      <SubscribeSheet open={sheet === "subscribe"} onClose={closeSheet} />
      <RestoreSheet open={sheet === "restore"} onClose={closeSheet} />
    </div>
  );
}
