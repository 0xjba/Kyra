import { lazy, Suspense, useEffect } from "react";
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
const Purge = lazy(() => import("./pages/Purge"));
const Installers = lazy(() => import("./pages/Installers"));
const Settings = lazy(() => import("./pages/Settings"));
const ModulePlaceholder = lazy(() => import("./pages/ModulePlaceholder"));

export default function App() {
  const loadSettings = useSettingsStore((s) => s.load);
  const settingsLoaded = useSettingsStore((s) => s.loaded);

  useEffect(() => {
    if (!settingsLoaded) loadSettings();
  }, [settingsLoaded, loadSettings]);

  return (
    <HashRouter>
      <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
        <TitleBar />
        <div style={{ height: 1, background: "rgba(255, 255, 255, 0.06)", flexShrink: 0 }} />
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
              <Route path="/purge" element={<Purge />} />
              <Route path="/installers" element={<Installers />} />
              <Route path="/settings" element={<Settings />} />
              <Route path="/:module" element={<ModulePlaceholder />} />
            </Routes>
          </Suspense>
        </div>
        <AccentBar />
      </div>
    </HashRouter>
  );
}
