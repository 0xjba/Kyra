import { useState, useEffect, useRef } from "react";
import kyraIcon from "../../src-tauri/icons/128x128.png";
import "../styles/fda-mockup.css";

export default function FdaMockup() {
  const [toggled, setToggled] = useState(false);
  const [cursorPhase, setCursorPhase] = useState<"idle" | "moving" | "clicking" | "done">("idle");
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const runCycle = () => {
      setCursorPhase("moving");
      setToggled(false);

      timeoutRef.current = setTimeout(() => {
        setCursorPhase("clicking");

        timeoutRef.current = setTimeout(() => {
          setToggled(true);
          setCursorPhase("done");

          timeoutRef.current = setTimeout(runCycle, 4500);
        }, 300);
      }, 1200);
    };

    timeoutRef.current = setTimeout(runCycle, 600);

    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []);

  return (
    <div className="fda-mockup">
      {/* Skeleton row above */}
      <div className="fda-row fda-row-skeleton">
        <div className="fda-skeleton-icon" />
        <div className="fda-skeleton-text" />
        <div className="fda-toggle off" />
      </div>

      {/* Kyra row — highlighted */}
      <div className={`fda-row fda-row-kyra ${toggled ? "granted" : ""}`}>
        <img src={kyraIcon} alt="Kyra" className="fda-app-icon-img" />
        <span className="fda-app-name fda-app-name-kyra">Kyra</span>
        <div className={`fda-toggle ${toggled ? "on" : "off"}`}>
          <div className="fda-toggle-knob" />
        </div>
      </div>

      {/* Skeleton row below */}
      <div className="fda-row fda-row-skeleton">
        <div className="fda-skeleton-icon" />
        <div className="fda-skeleton-text fda-skeleton-text-short" />
        <div className="fda-toggle off" />
      </div>

      {/* Animated cursor */}
      <div className={`fda-cursor fda-cursor-${cursorPhase}`}>
        <svg width="14" height="18" viewBox="0 0 12 16" fill="none">
          <path d="M1 1l0 12 3.5-3.5L7.5 15l2-1-3-5.5H11L1 1z" fill="white" stroke="black" strokeWidth="1" strokeLinejoin="round" />
        </svg>
        {cursorPhase === "clicking" && <div className="fda-cursor-ripple" />}
      </div>
    </div>
  );
}
