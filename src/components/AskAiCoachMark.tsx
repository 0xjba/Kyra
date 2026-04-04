import { useState, useEffect } from "react";

const STORAGE_KEY = "kyra_ask_ai_hint_seen";

/**
 * One-time coach mark that appears above the Ask AI button
 * on the first module visit. Dismissed globally (once for all modules).
 */
export default function AskAiCoachMark() {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!localStorage.getItem(STORAGE_KEY)) {
      // Small delay so the button is visible first
      const t = setTimeout(() => setVisible(true), 600);
      return () => clearTimeout(t);
    }
  }, []);

  if (!visible) return null;

  const dismiss = () => {
    setVisible(false);
    localStorage.setItem(STORAGE_KEY, "1");
  };

  return (
    <div className="coach-mark">
      <span className="coach-mark-close" onClick={dismiss}>✕</span>
      Not sure what's safe to remove? Ask AI for advice before you proceed.
    </div>
  );
}
