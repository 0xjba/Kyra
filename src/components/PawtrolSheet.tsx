import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import "../styles/pawtrol-sheet.css";

interface PawtrolSheetProps {
  label: string;
  onClose: () => void;
  children: ReactNode;
}

// Portaled so a transformed ancestor (onboarding slides, the titlebar) can't trap the fixed overlay.
export default function PawtrolSheet({ label, onClose, children }: PawtrolSheetProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <div className="paw-sheet-overlay" onClick={onClose}>
      <div
        className="paw-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
