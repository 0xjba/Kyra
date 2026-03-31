import { AlertTriangle } from "lucide-react";
import { useSettingsStore } from "../stores/settingsStore";
import "../styles/delete-dialog.css";

interface DeleteConfirmDialogProps {
  visible: boolean;
  title: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function DeleteConfirmDialog({
  visible,
  title,
  onConfirm,
  onCancel,
}: DeleteConfirmDialogProps) {
  const useTrash = useSettingsStore((s) => s.settings.use_trash);

  if (!visible) return null;

  return (
    <div className="delete-overlay" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
      <div className="delete-dialog">
        <div className="delete-dialog-icon">
          <AlertTriangle size={28} strokeWidth={1.6} />
        </div>

        <div className="delete-dialog-title">{title}</div>
        <div className="delete-dialog-desc">
          {useTrash
            ? "Files will be moved to Trash. You can recover them until Trash is emptied."
            : "This action cannot be undone. Files will be permanently removed."}
        </div>

        <div className="delete-dialog-buttons">
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button
            className={`btn ${useTrash ? "btn-primary" : "btn-danger"}`}
            onClick={onConfirm}
          >
            {useTrash ? "Move to Trash" : "Delete"}
          </button>
        </div>
      </div>
    </div>
  );
}
