import { HashRouter, Routes, Route } from "react-router-dom";
import AccentBar from "./components/AccentBar";
import TitleBar from "./components/TitleBar";
import Home from "./pages/Home";
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
            <Route path="/:module" element={<ModulePlaceholder />} />
          </Routes>
        </div>
      </div>
    </HashRouter>
  );
}
