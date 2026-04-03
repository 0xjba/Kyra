import { useState, useEffect, useRef, useCallback } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import { checkFullDiskAccess } from "../lib/tauri";
import { enable, disable } from "@tauri-apps/plugin-autostart";
import cat1 from "../assets/cat-tail/cat1.png";
import cat2 from "../assets/cat-tail/cat2.png";
import cat3 from "../assets/cat-tail/cat3.png";
import cat4 from "../assets/cat-tail/cat4.png";
import cat5 from "../assets/cat-tail/cat5.png";
import cat6 from "../assets/cat-tail/cat6.png";
import cat7 from "../assets/cat-tail/cat7.png";
import "../styles/onboarding.css";

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];

const STEPS = 5;

export default function Onboarding() {
  const [step, setStep] = useState(0);
  const [direction, setDirection] = useState<"forward" | "backward">("forward");
  const [fdaGranted, setFdaGranted] = useState<boolean | null>(null);
  const [notifStatus, setNotifStatus] = useState<"idle" | "granted" | "denied">("idle");

  // Local prefs — saved atomically on completion
  const [launchAtLogin, setLaunchAtLoginLocal] = useState(false);
  const [checkUpdates, setCheckUpdatesLocal] = useState(true);
  const [useTrash, setUseTrashLocal] = useState(false);
  const [notificationsEnabled, setNotificationsEnabledLocal] = useState(true);

  const setOnboardingCompleted = useSettingsStore((s) => s.setOnboardingCompleted);
  const setLaunchAtLogin = useSettingsStore((s) => s.setLaunchAtLogin);
  const setCheckForUpdates = useSettingsStore((s) => s.setCheckForUpdates);
  const setUseTrash = useSettingsStore((s) => s.setUseTrash);
  const setNotificationsEnabled = useSettingsStore((s) => s.setNotificationsEnabled);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [catFrame, setCatFrame] = useState(0);
  const [nextFrame, setNextFrame] = useState(1);
  const [fadeProgress, setFadeProgress] = useState(0);

  // Sprite animation — crossfade between frames
  useEffect(() => {
    if (step !== 0) return;
    const frameDuration = 220;
    const stepMs = 16;
    let elapsed = 0;
    let current = 0;

    const id = setInterval(() => {
      elapsed += stepMs;
      const linear = Math.min(elapsed / frameDuration, 1);
      const t = linear < 0.5 ? 2 * linear * linear : 1 - (-2 * linear + 2) ** 2 / 2;
      setFadeProgress(t);

      if (t >= 1) {
        elapsed = 0;
        current = (current + 1) % CAT_FRAMES.length;
        setCatFrame(current);
        setNextFrame((current + 1) % CAT_FRAMES.length);
        setFadeProgress(0);
      }
    }, stepMs);
    return () => clearInterval(id);
  }, [step]);

  // ── FDA polling ──
  const startFdaPoll = useCallback(() => {
    if (pollRef.current) return;
    checkFullDiskAccess().then((ok) => {
      setFdaGranted(ok);
      if (ok) return;
      let retries = 0;
      pollRef.current = setInterval(async () => {
        retries++;
        const granted = await checkFullDiskAccess();
        if (granted) {
          setFdaGranted(true);
          if (pollRef.current) clearInterval(pollRef.current);
          pollRef.current = null;
        }
        if (retries >= 60) {
          if (pollRef.current) clearInterval(pollRef.current);
          pollRef.current = null;
        }
      }, 2000);
    });
  }, []);

  useEffect(() => {
    if (step === 2) startFdaPoll();
    return () => {
      if (pollRef.current && step !== 2) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [step, startFdaPoll]);

  useEffect(() => {
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
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

  const handleRequestNotifs = async () => {
    try {
      const { isPermissionGranted, requestPermission } = await import(
        "@tauri-apps/plugin-notification"
      );
      let granted = await isPermissionGranted();
      if (!granted) {
        const result = await requestPermission();
        granted = result === "granted";
      }
      setNotifStatus(granted ? "granted" : "denied");
      setNotificationsEnabledLocal(granted);
    } catch {
      setNotifStatus("denied");
      setNotificationsEnabledLocal(false);
    }
  };

  const handleComplete = async () => {
    await setUseTrash(useTrash);
    await setCheckForUpdates(checkUpdates);
    await setNotificationsEnabled(notificationsEnabled);

    try {
      if (launchAtLogin) await enable();
      else await disable();
    } catch {}
    await setLaunchAtLogin(launchAtLogin);

    await setOnboardingCompleted(true);
  };

  const openFdaSettings = async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    invoke("open_fda_settings").catch(() => {});
  };

  // ── Screen 0: Welcome ──
  const renderWelcome = () => (
    <div className={`onboarding-slide ${direction}`} key="welcome">
      <div className="onboarding-hero">
        <div className="onboarding-sprite-container">
          <img src={CAT_FRAMES[catFrame]} alt="" className="onboarding-sprite" />
          <img
            src={CAT_FRAMES[nextFrame]}
            alt=""
            className="onboarding-sprite onboarding-sprite-next"
            style={{ opacity: fadeProgress }}
          />
        </div>
        <div className="onboarding-title">Kyra</div>
      </div>
      <div className="onboarding-tagline">Nine lives for your storage</div>
    </div>
  );

  // ── Screen 1: Features ──
  const renderFeatures = () => (
    <div className={`onboarding-slide ${direction}`} key="features">
      <div className="onboarding-heading">What Kyra Does</div>
      <div className="onboarding-desc">
        Everything you need to keep your Mac fast, clean, and clutter-free.
      </div>

      <div className="onboarding-features-grid">
        <div className="onboarding-feature">
          <div className="onboarding-feature-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M3 6h18M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2" />
            </svg>
          </div>
          <div className="onboarding-feature-name">Clean</div>
          <div className="onboarding-feature-desc">Remove caches, logs & junk files</div>
        </div>

        <div className="onboarding-feature">
          <div className="onboarding-feature-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z" />
            </svg>
          </div>
          <div className="onboarding-feature-name">Optimize</div>
          <div className="onboarding-feature-desc">Speed up your Mac in one click</div>
        </div>

        <div className="onboarding-feature">
          <div className="onboarding-feature-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
              <path d="M8 21h8M12 17v4" />
            </svg>
          </div>
          <div className="onboarding-feature-name">Monitor</div>
          <div className="onboarding-feature-desc">Real-time CPU, memory & disk stats</div>
        </div>

        <div className="onboarding-feature">
          <div className="onboarding-feature-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 4H8l-7 8 7 8h13a2 2 0 002-2V6a2 2 0 00-2-2z" />
              <line x1="18" y1="9" x2="12" y2="15" />
              <line x1="12" y1="9" x2="18" y2="15" />
            </svg>
          </div>
          <div className="onboarding-feature-name">Uninstall</div>
          <div className="onboarding-feature-desc">Fully remove apps & leftovers</div>
        </div>

        <div className="onboarding-feature">
          <div className="onboarding-feature-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="11" cy="11" r="8" />
              <path d="M21 21l-4.35-4.35" />
            </svg>
          </div>
          <div className="onboarding-feature-name">Analyze</div>
          <div className="onboarding-feature-desc">Visualize what's eating your disk</div>
        </div>

        <div className="onboarding-feature">
          <div className="onboarding-feature-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M14.7 6.3a1 1 0 000 1.4l1.6 1.6a1 1 0 001.4 0l3.77-3.77a6 6 0 01-7.94 7.94l-6.91 6.91a2.12 2.12 0 01-3-3l6.91-6.91a6 6 0 017.94-7.94l-3.76 3.76z" />
            </svg>
          </div>
          <div className="onboarding-feature-name">Purge</div>
          <div className="onboarding-feature-desc">Delete dev build artifacts</div>
        </div>
      </div>
    </div>
  );

  // ── Screen 2: Full Disk Access ──
  const renderFda = () => (
    <div className={`onboarding-slide ${direction}`} key="fda">
      <div className="onboarding-heading">Full Disk Access</div>
      <div className="onboarding-desc">
        To scan protected areas like browser caches and system logs, Kyra needs
        Full Disk Access.
      </div>

      <div className="onboarding-steps">
        <div className="onboarding-step">
          <span className="onboarding-step-num">1</span>
          <span>Click "Open System Settings" below</span>
        </div>
        <div className="onboarding-step">
          <span className="onboarding-step-num">2</span>
          <span>Find Kyra in the list and toggle it on</span>
        </div>
        <div className="onboarding-step">
          <span className="onboarding-step-num">3</span>
          <span>Come back here — we'll detect it automatically</span>
        </div>
      </div>

      <button className="btn btn-primary" onClick={openFdaSettings}>
        Open System Settings
      </button>

      <div className="onboarding-status">
        <div
          className={`onboarding-status-dot ${fdaGranted ? "granted" : "pending"}`}
        />
        <span className="onboarding-status-text">
          {fdaGranted === null
            ? "Checking..."
            : fdaGranted
              ? "Access Granted"
              : "Not Yet Granted"}
        </span>
      </div>
    </div>
  );

  // ── Screen 3: Quick Setup (preferences + notifications) ──
  const renderSetup = () => (
    <div className={`onboarding-slide ${direction}`} key="setup">
      <div className="onboarding-heading">Quick Setup</div>
      <div className="onboarding-prefs-note">You can always change these in Settings</div>

      <div className="onboarding-prefs-card">
        <label className="settings-row">
          <div className="settings-row-info">
            <div className="settings-row-name">Launch at Login</div>
            <div className="settings-row-desc">Start Kyra when you log in</div>
          </div>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={launchAtLogin}
            onChange={(e) => setLaunchAtLoginLocal(e.target.checked)}
          />
        </label>
        <label className="settings-row">
          <div className="settings-row-info">
            <div className="settings-row-name">Check for Updates</div>
            <div className="settings-row-desc">Automatically check on launch</div>
          </div>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={checkUpdates}
            onChange={(e) => setCheckUpdatesLocal(e.target.checked)}
          />
        </label>
        <label className="settings-row">
          <div className="settings-row-info">
            <div className="settings-row-name">Move to Trash</div>
            <div className="settings-row-desc">Send files to Trash instead of deleting</div>
          </div>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={useTrash}
            onChange={(e) => setUseTrashLocal(e.target.checked)}
          />
        </label>
        <div className="settings-row">
          <div className="settings-row-info">
            <div className="settings-row-name">Notifications</div>
            <div className="settings-row-desc">Low disk space & update alerts</div>
          </div>
          {notifStatus === "idle" ? (
            <button className="btn settings-btn-sm" onClick={handleRequestNotifs}>
              Enable
            </button>
          ) : (
            <span className={`onboarding-inline-status ${notifStatus === "granted" ? "granted" : ""}`}>
              {notifStatus === "granted" ? "Enabled" : "Denied"}
            </span>
          )}
        </div>
      </div>
    </div>
  );

  // ── Screen 4: Done ──
  const renderDone = () => (
    <div className={`onboarding-slide ${direction}`} key="done">
      <div className="onboarding-check-circle">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
          <path className="onboarding-check-path" d="M5 12l5 5L19 7" />
        </svg>
      </div>
      <div className="onboarding-heading">You're All Set</div>
      <div className="onboarding-desc">Kyra is ready to keep your Mac clean and fast.</div>

      <div className="onboarding-summary">
        <div className="onboarding-summary-row">
          <span className="onboarding-summary-label">Full Disk Access</span>
          <span className="onboarding-summary-value">
            {fdaGranted ? "Granted" : "Limited"}
          </span>
        </div>
        <div className="onboarding-summary-row">
          <span className="onboarding-summary-label">Notifications</span>
          <span className="onboarding-summary-value">
            {notifStatus === "granted" ? "Enabled" : "Skipped"}
          </span>
        </div>
        <div className="onboarding-summary-row">
          <span className="onboarding-summary-label">Launch at Login</span>
          <span className="onboarding-summary-value">
            {launchAtLogin ? "On" : "Off"}
          </span>
        </div>
        <div className="onboarding-summary-row">
          <span className="onboarding-summary-label">Auto Updates</span>
          <span className="onboarding-summary-value">
            {checkUpdates ? "On" : "Off"}
          </span>
        </div>
      </div>
    </div>
  );

  const screens = [renderWelcome, renderFeatures, renderFda, renderSetup, renderDone];

  return (
    <div className="onboarding-container">
      <div className="onboarding-content">
        {screens[step]()}
      </div>

      <div className="onboarding-footer">
        <div className="onboarding-dots">
          {Array.from({ length: STEPS }).map((_, i) => (
            <div
              key={i}
              className={`onboarding-dot ${i === step ? "active" : ""}`}
            />
          ))}
        </div>

        <div className="onboarding-nav">
          {step > 0 && step < STEPS - 1 && (
            <button className="btn" onClick={goBack}>
              Back
            </button>
          )}
          {step === 0 && (
            <button className="btn btn-primary" onClick={goNext}>
              Get Started
            </button>
          )}
          {step > 0 && step < STEPS - 1 && (
            <button className="btn btn-primary" onClick={goNext}>
              Next
            </button>
          )}
          {step === STEPS - 1 && (
            <button className="btn btn-primary" onClick={handleComplete}>
              Open Kyra
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
