import { useEffect, useState, type FormEvent } from "react";
import { ExternalLink, Mail } from "lucide-react";
import { useGuardianStore } from "../stores/guardianStore";
import type { PawtrolPlan } from "../lib/tauri";
import { isValidEmail } from "../utils/pawtrolAccount";
import PawtrolSheet from "./PawtrolSheet";
import "../styles/guardian.css";

const PLANS: { value: PawtrolPlan; label: string; price: string; hint?: string }[] = [
  { value: "yearly", label: "Yearly", price: "$9.99/year", hint: "Save 16%" },
  { value: "monthly", label: "Monthly", price: "$0.99/month" },
];

interface SubscribeSheetProps {
  open: boolean;
  onClose: () => void;
}

export default function SubscribeSheet({ open, onClose }: SubscribeSheetProps) {
  if (!open) return null;
  return <SubscribeFlow onClose={onClose} />;
}

function SubscribeFlow({ onClose }: { onClose: () => void }) {
  const subscribe = useGuardianStore((s) => s.subscribe);
  const subscribeError = useGuardianStore((s) => s.subscribeError);
  const polling = useGuardianStore((s) => s.checkoutPolling);
  const licensed = useGuardianStore((s) => s.license.active);
  const checkLicense = useGuardianStore((s) => s.checkLicense);

  const [step, setStep] = useState<"email" | "checkout">("email");
  const [email, setEmail] = useState("");
  const [plan, setPlan] = useState<PawtrolPlan>("yearly");
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [notYet, setNotYet] = useState(false);

  useEffect(() => {
    useGuardianStore.setState({ subscribeError: null });
  }, []);

  useEffect(() => {
    if (licensed && step === "checkout") onClose();
  }, [licensed, step, onClose]);

  const valid = isValidEmail(email);
  const emailError = touched && !valid ? "Enter a valid email address." : null;

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    setTouched(true);
    if (!valid || busy) return;
    setBusy(true);
    const ok = await subscribe(email, plan);
    setBusy(false);
    if (ok) setStep("checkout");
  };

  const refresh = async () => {
    if (checking) return;
    setChecking(true);
    setNotYet(false);
    await checkLicense();
    setChecking(false);
    if (!useGuardianStore.getState().license.active) setNotYet(true);
  };

  if (step === "checkout") {
    return (
      <PawtrolSheet label="Finish checkout in your browser" onClose={onClose}>
        <div className="paw-sheet-icon">
          <ExternalLink size={22} strokeWidth={2} />
        </div>
        <div className="paw-sheet-title">Finish checkout in your browser</div>
        <div className="paw-sheet-desc">
          We opened a secure checkout for <strong>{email.trim()}</strong>. Pawtrol unlocks here on its own
          once you've paid.
        </div>
        {polling ? (
          <div className="paw-sheet-status" role="status">
            <span className="paw-sheet-status-dot" />
            Waiting for payment…
          </div>
        ) : (
          <div className="paw-sheet-status" role="status">Paid already? Refresh to unlock.</div>
        )}
        {notYet && (
          <div className="paw-sheet-error">Not active yet. It can take a minute after paying.</div>
        )}
        <div className="paw-sheet-actions">
          <button type="button" className="paw-sheet-btn" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="paw-sheet-btn paw-sheet-btn-primary" onClick={refresh} disabled={checking}>
            {checking ? "Checking…" : "I've subscribed, refresh"}
          </button>
        </div>
      </PawtrolSheet>
    );
  }

  const error = emailError ?? subscribeError;
  return (
    <PawtrolSheet label="Subscribe to Pawtrol" onClose={onClose}>
      <form className="paw-sheet-form" onSubmit={submit} noValidate>
        <div className="paw-sheet-icon">
          <Mail size={22} strokeWidth={2} />
        </div>
        <div className="paw-sheet-title">Subscribe to Pawtrol</div>
        <div className="paw-sheet-desc">
          We'll send your receipt here and use it to restore Pawtrol on another Mac.
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
        <div className="gd-seg paw-sheet-plans" role="radiogroup" aria-label="Plan">
          {PLANS.map((p) => {
            const on = p.value === plan;
            return (
              <button
                key={p.value}
                type="button"
                role="radio"
                aria-checked={on}
                className={`gd-seg-opt${on ? " on" : ""}`}
                onClick={() => setPlan(p.value)}
              >
                {p.price}
                {p.hint && <span className="paw-sheet-plan-hint">{p.hint}</span>}
              </button>
            );
          })}
        </div>
        {error && (
          <div className="paw-sheet-error" role="alert">
            {error}
          </div>
        )}
        <div className="paw-sheet-actions">
          <button type="button" className="paw-sheet-btn" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="paw-sheet-btn paw-sheet-btn-primary" disabled={busy}>
            {busy ? "Opening checkout…" : `Continue · ${PLANS.find((p) => p.value === plan)!.price}`}
          </button>
        </div>
      </form>
    </PawtrolSheet>
  );
}
