import { useEffect, type ReactNode } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import catImg from "../assets/cat.png";
import "../styles/delete-dialog.css";

interface DeleteConfirmDialogProps {
  visible: boolean;
  title: string;
  onConfirm: () => void;
  onCancel: () => void;
  description?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
}

export default function DeleteConfirmDialog({
  visible,
  title,
  onConfirm,
  onCancel,
  description,
  confirmLabel,
  cancelLabel = "Cancel",
  destructive,
}: DeleteConfirmDialogProps) {
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  useEffect(() => {
    if (!visible) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, onCancel]);

  if (!visible) return null;

  const isDestructive = destructive ?? !useTrash;
  const desc =
    description ??
    (useTrash
      ? "Files will be moved to the Trash. You can put them back until the Trash is emptied."
      : "Files are deleted permanently. Turn on Move to Trash in Settings if you'd rather have a safety net.");
  const label = confirmLabel ?? (useTrash ? "Move to Trash" : "Delete");

  return (
    <div className="delete-overlay" onClick={onCancel}>
      <div
        className="delete-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <img src={catImg} alt="" className="delete-dialog-cat" draggable={false} />
        <div className="delete-dialog-title">{title}</div>
        <div className="delete-dialog-desc">{desc}</div>
        <div className="delete-dialog-buttons">
          <button type="button" className="delete-dialog-btn" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={`delete-dialog-btn ${isDestructive ? "delete-dialog-btn-danger" : "delete-dialog-btn-primary"}`}
            onClick={onConfirm}
          >
            {label}
          </button>
        </div>
      </div>
    </div>
  );
}
