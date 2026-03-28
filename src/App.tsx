import { HashRouter, Routes, Route } from "react-router-dom";
import AccentBar from "./components/AccentBar";
import TitleBar from "./components/TitleBar";
import Home from "./pages/Home";
import Clean from "./pages/Clean";
import Optimize from "./pages/Optimize";
import Uninstall from "./pages/Uninstall";
import Analyze from "./pages/Analyze";
import Status from "./pages/Status";
import Purge from "./pages/Purge";
import Installers from "./pages/Installers";
import ModulePlaceholder from "./pages/ModulePlaceholder";

export default function App() {
  return (
    <HashRouter>
      <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
        <AccentBar />
        <TitleBar />
        <div style={{ flex: 1, overflow: "hidden" }}>
          <Routes>
            <Route path="/" element={<Home />} />
            <Route path="/clean" element={<Clean />} />
            <Route path="/optimize" element={<Optimize />} />
            <Route path="/uninstall" element={<Uninstall />} />
            <Route path="/analyze" element={<Analyze />} />
            <Route path="/status" element={<Status />} />
            <Route path="/purge" element={<Purge />} />
            <Route path="/installers" element={<Installers />} />
            <Route path="/:module" element={<ModulePlaceholder />} />
          </Routes>
        </div>
      </div>
    </HashRouter>
  );
}
