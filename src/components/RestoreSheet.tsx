import { useEffect, useRef, useState, type FormEvent } from "react";
import { KeyRound, RotateCcw } from "lucide-react";
import { useGuardianStore } from "../stores/guardianStore";
import {
  RESEND_COOLDOWN_SECS,
  RESTORE_CODE_LENGTH,
  errorText,
  isValidEmail,
  sanitizeCode,
} from "../utils/pawtrolAccount";
import PawtrolSheet from "./PawtrolSheet";
import catImg from "../assets/cat.png";

interface RestoreSheetProps {
  open: boolean;
  onClose: () => void;
}

const NO_SUBSCRIPTION = "We couldn't find an active Pawtrol subscription for that email.";

export default function RestoreSheet({ open, onClose }: RestoreSheetProps) {
  if (!open) return null;
  return <RestoreFlow onClose={onClose} />;
}

function RestoreFlow({ onClose }: { onClose: () => void }) {
  const restoreStart = useGuardianStore((s) => s.restoreStart);
  const restoreVerify = useGuardianStore((s) => s.restoreVerify);

  const [step, setStep] = useState<"email" | "code" | "done">("email");
  const [email, setEmail] = useState("");
  const [touched, setTouched] = useState(false);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [resendIn, setResendIn] = useState(0);
  const codeRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (step !== "code" || resendIn <= 0) return;
    const id = setTimeout(() => setResendIn((n) => n - 1), 1000);
    return () => clearTimeout(id);
  }, [step, resendIn]);

  const validEmail = isValidEmail(email);
  const emailError = touched && !validEmail ? "Enter a valid email address." : null;

  const sendCode = async (e?: FormEvent) => {
    e?.preventDefault();
    setTouched(true);
    if (!validEmail || busy) return;
    setBusy(true);
    setError(null);
    try {
      await restoreStart(email);
      setStep("code");
      setCode("");
      setResendIn(RESEND_COOLDOWN_SECS);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const resend = async () => {
    if (busy || resendIn > 0) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await restoreStart(email);
      setNotice("We sent a new code.");
      setResendIn(RESEND_COOLDOWN_SECS);
      setCode("");
      codeRef.current?.focus();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const verify = async (value: string) => {
    if (busy || value.length !== RESTORE_CODE_LENGTH) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      if (await restoreVerify(email, value)) {
        setStep("done");
        return;
      }
      setError(NO_SUBSCRIPTION);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
    setCode("");
    codeRef.current?.focus();
  };

  const onCodeChange = (raw: string) => {
    const next = sanitizeCode(raw);
    setCode(next);
    if (error) setError(null);
    if (next.length === RESTORE_CODE_LENGTH) verify(next);
  };

  if (step === "done") {
    return (
      <PawtrolSheet label="Pawtrol is back on this Mac" onClose={onClose}>
        <img src={catImg} alt="" className="paw-sheet-cat" draggable={false} />
        <div className="paw-sheet-title">Pawtrol is back on this Mac</div>
        <div className="paw-sheet-desc">Your subscription is active here. Pawtrol will start patrolling on its own.</div>
        <div className="paw-sheet-actions">
          <button type="button" className="paw-sheet-btn paw-sheet-btn-primary" onClick={onClose} autoFocus>
            Done
          </button>
        </div>
      </PawtrolSheet>
    );
  }

  if (step === "code") {
    const mins = Math.floor(resendIn / 60);
    const secs = String(resendIn % 60).padStart(2, "0");
    return (
      <PawtrolSheet label="Enter your code" onClose={onClose}>
        <form
          className="paw-sheet-form"
          noValidate
          onSubmit={(e) => {
            e.preventDefault();
            verify(code);
          }}
        >
          <div className="paw-sheet-icon">
            <KeyRound size={22} strokeWidth={2} />
          </div>
          <div className="paw-sheet-title">Check your email</div>
          <div className="paw-sheet-desc">
            Enter the 6-digit code we sent to <strong>{email.trim()}</strong>. It's valid for 10 minutes.
          </div>
          <input
            ref={codeRef}
            className="paw-sheet-field paw-sheet-code"
            type="text"
            inputMode="numeric"
            autoComplete="one-time-code"
            aria-label="6-digit code"
            placeholder="000000"
            autoFocus
            value={code}
            aria-invalid={!!error}
            readOnly={busy}
            onChange={(e) => onCodeChange(e.target.value)}
          />
          {error && (
            <div className="paw-sheet-error" role="alert">
              {error}
            </div>
          )}
          {!error && notice && <div className="paw-sheet-status" role="status">{notice}</div>}
          <div className="paw-sheet-links">
            <button type="button" className="paw-sheet-link" onClick={resend} disabled={busy || resendIn > 0}>
              {resendIn > 0 ? `Resend code in ${mins}:${secs}` : "Resend code"}
            </button>
            <button
              type="button"
              className="paw-sheet-link"
              onClick={() => {
                setStep("email");
                setError(null);
                setNotice(null);
              }}
            >
              Use a different email
            </button>
          </div>
          <div className="paw-sheet-actions">
            <button type="button" className="paw-sheet-btn" onClick={onClose}>
              Cancel
            </button>
            <button
              type="submit"
              className="paw-sheet-btn paw-sheet-btn-primary"
              disabled={busy || code.length !== RESTORE_CODE_LENGTH}
            >
              {busy ? "Checking…" : "Verify"}
            </button>
          </div>
        </form>
      </PawtrolSheet>
    );
  }

  const shown = emailError ?? error;
  return (
    <PawtrolSheet label="Restore Pawtrol" onClose={onClose}>
      <form className="paw-sheet-form" onSubmit={sendCode} noValidate>
        <div className="paw-sheet-icon">
          <RotateCcw size={22} strokeWidth={2} />
        </div>
        <div className="paw-sheet-title">Restore Pawtrol</div>
        <div className="paw-sheet-desc">
          Enter the email you subscribed with and we'll send you a code to turn Pawtrol on for this Mac.
        </div>
        <input
          className="paw-sheet-field"
          type="email"
          aria-label="Email"
          placeholder="you@example.com"
          autoComplete="email"
          autoCapitalize="off"
          spellCheck={false}
          autoFocus
          value={email}
          aria-invalid={!!emailError}
          onChange={(e) => setEmail(e.target.value)}
          onBlur={() => email && setTouched(true)}
        />
        {shown && (
          <div className="paw-sheet-error" role="alert">
            {shown}
          </div>
        )}
        <div className="paw-sheet-actions">
          <button type="button" className="paw-sheet-btn" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="paw-sheet-btn paw-sheet-btn-primary" disabled={busy}>
            {busy ? "Sending…" : "Send code"}
          </button>
        </div>
      </form>
    </PawtrolSheet>
  );
}
