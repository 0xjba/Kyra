import { useEffect, useState } from "react";
import { Check } from "lucide-react";
import "../styles/demo-player.css";

const DEMO_ROWS: [string, number][] = [
  ["System caches", 8.2],
  ["Chrome cache", 4.1],
  ["Xcode DerivedData", 7.4],
  ["Old installers", 3.3],
];

const DISK_TOTAL = 249;
const DISK_START = 214;

export default function DemoPlayer() {
  const [tick, setTick] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setTick((t) => (t + 1) % (DEMO_ROWS.length + 2)), 900);
    return () => clearInterval(id);
  }, []);

  const cleaned = DEMO_ROWS.slice(0, Math.min(tick, DEMO_ROWS.length)).reduce((a, [, gb]) => a + gb, 0);
  const used = Math.round(DISK_START - cleaned);

  return (
    <div className="demo-panel">
      <div className="demo-disk">
        <span className="demo-disk-name">Macintosh HD</span>
        <span className="demo-disk-used">{used} GB used of {DISK_TOTAL} GB</span>
      </div>
      <div className="demo-bar">
        <div className="demo-bar-fill" style={{ width: `${(used / DISK_TOTAL) * 100}%` }}>
          <span style={{ flex: 3, background: "#FD4841" }} />
          <span style={{ flex: 2, background: "#FDD225" }} />
          <span style={{ flex: 2, background: "#2AC852" }} />
          <span style={{ flex: 2, background: "#22B8F0" }} />
        </div>
      </div>
      {DEMO_ROWS.map(([label, gb], i) => {
        const done = i < tick;
        return (
          <div key={label} className={`demo-row${done ? " done" : ""}`}>
            <span className={`demo-check${done ? " done" : ""}`}>
              <Check size={11} strokeWidth={3} />
            </span>
            <span className="demo-row-label">{label}</span>
            <span className="demo-row-size">{gb} GB</span>
          </div>
        );
      })}
    </div>
  );
}
