import { lazy, Suspense, useEffect, useRef } from "react";
import { HashRouter, Routes, Route } from "react-router-dom";
import { useSettingsStore } from "./stores/settingsStore";
import AccentBar from "./components/AccentBar";
import TitleBar from "./components/TitleBar";
import FdaPrompt from "./components/FdaPrompt";

const Home = lazy(() => import("./pages/Home"));
const Clean = lazy(() => import("./pages/Clean"));
const Optimize = lazy(() => import("./pages/Optimize"));
const Uninstall = lazy(() => import("./pages/Uninstall"));
const Analyze = lazy(() => import("./pages/Analyze"));
const Status = lazy(() => import("./pages/Status"));
const Prune = lazy(() => import("./pages/Prune"));
const Installers = lazy(() => import("./pages/Installers"));
const Settings = lazy(() => import("./pages/Settings"));
const ModulePlaceholder = lazy(() => import("./pages/ModulePlaceholder"));
const Onboarding = lazy(() => import("./pages/Onboarding"));

const LOW_DISK_KEY = "kyra_last_low_disk_warn";

async function checkLowDiskSpace(thresholdGb: number) {
  try {
    const { getSystemStats } = await import("./lib/tauri");
    const stats = await getSystemStats();
    const freeGb = stats.disk_free / (1024 * 1024 * 1024);
    if (freeGb < thresholdGb) {
      // Only warn once per day
      const lastWarn = localStorage.getItem(LOW_DISK_KEY);
      const now = new Date().toDateString();
      if (lastWarn === now) return;
      localStorage.setItem(LOW_DISK_KEY, now);

      const { sendNotification, isPermissionGranted, requestPermission } =
        await import("@tauri-apps/plugin-notification");
      let permitted = await isPermissionGranted();
      if (!permitted) {
        const result = await requestPermission();
        permitted = result === "granted";
      }
      if (permitted) {
        sendNotification({
          title: "Low Disk Space",
          body: `Only ${freeGb.toFixed(1)} GB free. Consider cleaning up with Kyra.`,
        });
      }
    }
  } catch {
    // Silently fail — don't disrupt app launch
  }
}

async function checkForAppUpdate() {
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    await check();
    // Update availability is handled in Settings page UI
  } catch {
    // Silently fail
  }
}

export default function App() {
  const loadSettings = useSettingsStore((s) => s.load);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const settings = useSettingsStore((s) => s.settings);
  const startupChecked = useRef(false);

  useEffect(() => {
    if (!settingsLoaded) loadSettings();
  }, [settingsLoaded, loadSettings]);

  // Run startup checks once settings are loaded (only after onboarding)
  useEffect(() => {
    if (!settingsLoaded || startupChecked.current || !settings.onboarding_completed) return;
    startupChecked.current = true;

    if (settings.notifications_enabled) {
      checkLowDiskSpace(settings.low_disk_threshold_gb);
    }
    if (settings.check_for_updates) {
      checkForAppUpdate();
    }
  }, [settingsLoaded, settings.onboarding_completed, settings.notifications_enabled, settings.check_for_updates, settings.low_disk_threshold_gb]);

  const showOnboarding = settingsLoaded && !settings.onboarding_completed;

  return (
    <HashRouter>
      <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
        <TitleBar />
        <div style={{ height: 1, background: "rgba(255, 255, 255, 0.06)", flexShrink: 0 }} />
        {showOnboarding ? (
          <Suspense fallback={null}>
            <Onboarding />
          </Suspense>
        ) : (
          <>
            <FdaPrompt />
            <div style={{ flex: 1, overflow: "hidden" }}>
              <Suspense fallback={null}>
                <Routes>
                  <Route path="/" element={<Home />} />
                  <Route path="/clean" element={<Clean />} />
                  <Route path="/optimize" element={<Optimize />} />
                  <Route path="/uninstall" element={<Uninstall />} />
                  <Route path="/analyze" element={<Analyze />} />
                  <Route path="/status" element={<Status />} />
                  <Route path="/prune" element={<Prune />} />
                  <Route path="/installers" element={<Installers />} />
                  <Route path="/settings" element={<Settings />} />
                  <Route path="/:module" element={<ModulePlaceholder />} />
                </Routes>
              </Suspense>
            </div>
          </>
        )}
        <AccentBar />
      </div>
    </HashRouter>
  );
}
