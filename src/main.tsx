import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/global.css";
import "./styles/dark-mode.css";
import { startSummaryTracking } from "./stores/summaryStore";

startSummaryTracking();

// Disable browser right-click context menu for desktop app
document.addEventListener('contextmenu', (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
