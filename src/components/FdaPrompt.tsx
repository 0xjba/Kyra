import { useState, useEffect, useRef } from "react";
import { checkFullDiskAccess } from "../lib/tauri";
import { invoke } from "@tauri-apps/api/core";
import catImg from "../assets/cat-tail/cat1.png";
import "../styles/fda-prompt.css";

let skippedThisLaunch = false;

/** Suppress the prompt until the next launch (the user just skipped FDA in onboarding). */
export function skipFdaPromptThisLaunch() {
  skippedThisLaunch = true;
}

export default function FdaPrompt() {
  const [hasAccess, setHasAccess] = useState<boolean | null>(null);
  const [dismissed, setDismissed] = useState(skippedThisLaunch);
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
    <div className="fda-prompt-overlay">
      <div className="fda-prompt">
        <img src={catImg} alt="" className="fda-prompt-cat" />
        <div className="fda-prompt-title">Kyra needs Full Disk Access</div>
        <div className="fda-prompt-desc">
          Caches, logs and browser data live in protected folders. Without access, Kyra
          can only clean what macOS leaves open.
        </div>

        <ol className="fda-prompt-steps">
          <li><span className="fda-prompt-step-num">1</span>Open Settings</li>
          <li><span className="fda-prompt-step-num">2</span>Turn on Kyra under Full Disk Access</li>
          <li><span className="fda-prompt-step-num">3</span>Kyra restarts on its own</li>
        </ol>

        <button className="fda-prompt-btn fda-prompt-btn-primary" onClick={() => invoke("open_fda_settings")}>
          Open Settings
        </button>
        <div className="fda-prompt-row">
          <button className="fda-prompt-btn" onClick={() => setDismissed(true)}>
            Not now
          </button>
          <button className="fda-prompt-btn" onClick={() => invoke("restart_app")}>
            Restart Kyra
          </button>
        </div>
      </div>
    </div>
  );
}
