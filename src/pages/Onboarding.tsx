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
import walk1 from "../assets/cat-walking/cat_walking_01.png";
import walk2 from "../assets/cat-walking/cat_walking_02.png";
import walk3 from "../assets/cat-walking/cat_walking_03.png";
import walk4 from "../assets/cat-walking/cat_walking_04.png";
import walk5 from "../assets/cat-walking/cat_walking_05.png";
import DemoPlayer, { DEMO_STORAGE_SEGMENTS } from "../components/DemoPlayer";
import FdaMockup from "../components/FdaMockup";
import "../styles/onboarding.css";
import "../styles/settings.css";

const CAT_FRAMES = [cat1, cat2, cat3, cat4, cat5, cat6, cat7, cat6, cat5, cat4, cat3, cat2];
const WALK_FRAMES = [walk1, walk2, walk3, walk4, walk5, walk4, walk3, walk2];

const STEPS = 5;

export default function Onboarding() {
  const [step, setStep] = useState(0);
  const [direction, setDirection] = useState<"forward" | "backward">("forward");
  const [fdaGranted, setFdaGranted] = useState<boolean | null>(null);
  const [demoStorageUsed, setDemoStorageUsed] = useState(249);

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
  const [walkFrame, setWalkFrame] = useState(0);

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

  // Walk sprite animation
  useEffect(() => {
    if (step !== 4) return;
    const id = setInterval(() => {
      setWalkFrame((f) => (f + 1) % WALK_FRAMES.length);
    }, 120);
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

  // ── Screen 1: Features (Live Demo) ──
  const renderFeatures = () => {
    const diskTotal = 249;
    const freeGB = diskTotal - demoStorageUsed;
    const freePct = Math.max((freeGB / diskTotal) * 100, 0);

    return (
      <div className={`onboarding-slide ${direction}`} key="features">
        <div className="onboarding-heading">See Kyra in Action</div>
        <div className="onboarding-desc">
          Watch how Kyra frees up your storage in seconds.
        </div>

        <div className="onboarding-storage-wrapper">
          <div className="onboarding-storage-track">
            {DEMO_STORAGE_SEGMENTS.map((seg, i) => {
              const pct = (seg.ratio * demoStorageUsed / diskTotal) * 100;
              return (
                <div
                  key={seg.label}
                  className="onboarding-storage-segment"
                  style={{
                    width: `${Math.max(pct, 0.5)}%`,
                    background: `linear-gradient(90deg, color-mix(in srgb, ${seg.color}, white 25%) 0%, ${seg.color} 100%)`,
                    borderRadius:
                      i === 0 && i === DEMO_STORAGE_SEGMENTS.length - 1
                        ? "4px"
                        : i === 0
                          ? "4px 0 0 4px"
                          : i === DEMO_STORAGE_SEGMENTS.length - 1
                            ? "0 4px 4px 0"
                            : "0",
                  }}
                />
              );
            })}
            <div
              className="onboarding-storage-segment onboarding-storage-free"
              style={{ width: `${freePct}%`, borderRadius: "0 4px 4px 0" }}
            />
          </div>
          <div className="onboarding-storage-info">
            <span className="onboarding-storage-disk">Macintosh HD</span>
            <span className="onboarding-storage-label">{demoStorageUsed} GB used of {diskTotal} GB</span>
          </div>
        </div>

        <DemoPlayer onStorageChange={setDemoStorageUsed} />
      </div>
    );
  };

  // ── Screen 2: Full Disk Access ──
  const renderFda = () => (
    <div className={`onboarding-slide ${direction}`} key="fda">
      <div className="onboarding-heading">Full Disk Access</div>
      <div className="onboarding-desc">
        To scan protected areas like browser caches and system logs, Kyra needs
        Full Disk Access.
      </div>

      <FdaMockup />

      <div className="onboarding-fda-hint">
        Open System Settings, scroll to see Kyra &amp; enable it
      </div>

      <button
        className="btn btn-primary"
        onClick={fdaGranted ? undefined : openFdaSettings}
        disabled={!!fdaGranted}
      >
        {fdaGranted ? "Access Granted" : "Open System Settings"}
      </button>
    </div>
  );

  // ── Screen 3: Quick Setup (preferences + notifications) ──
  const renderSetup = () => (
    <div className={`onboarding-slide ${direction}`} key="setup">
      <div className="onboarding-heading">Quick Setup</div>
      <div className="onboarding-prefs-note">You can always change these in Settings</div>

      <div className="onboarding-prefs-list">
        <label className="onboarding-pref-item">
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
        <label className="onboarding-pref-item">
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
        <label className="onboarding-pref-item">
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
        <label className="onboarding-pref-item">
          <div className="settings-row-info">
            <div className="settings-row-name">Notifications</div>
            <div className="settings-row-desc">Low disk space & update alerts</div>
          </div>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={notificationsEnabled}
            onChange={(e) => setNotificationsEnabledLocal(e.target.checked)}
          />
        </label>
      </div>
    </div>
  );

  // ── Screen 4: Done ──
  const renderDone = () => (
    <div className={`onboarding-slide ${direction}`} key="done">
      <div className="onboarding-walk-cat">
        <img src={WALK_FRAMES[walkFrame]} alt="Kyra walking" />
      </div>
      <div className="onboarding-heading">Kyra's on paw-trol</div>
      <div className="onboarding-desc">
        She grooms your caches clean. Hunts down stale builds.
        {"\n"}Chases out forgotten apps & keeps one eye on your disk
        {"\n"}Purr-manently.
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
              {step === 2 && !fdaGranted ? "Skip" : "Next"}
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
