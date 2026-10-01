import type { Account } from "../lib/tauri";

export const PAWTROL_MAX_DEVICES = 3;
export const RESTORE_CODE_LENGTH = 6;
export const RESEND_COOLDOWN_SECS = 30;

const EMAIL_RE = /^[^\s@]+@[^\s@.]+(\.[^\s@.]+)+$/;

export function isValidEmail(email: string): boolean {
  return EMAIL_RE.test(email.trim());
}

/** Keeps digits only, so pasting "123 456" or "Your code: 123456" still works. */
export function sanitizeCode(input: string): string {
  return input.replace(/\D/g, "").slice(0, RESTORE_CODE_LENGTH);
}

export function formatPlanDate(secs: number): string {
  return new Date(secs * 1000).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

export const PAWTROL_PRICES = "$0.99/month or $9.99/year";

export function planLabel(plan: Account["plan"]): string | null {
  if (plan === "yearly") return "Yearly plan";
  if (plan === "monthly") return "Monthly plan";
  return null;
}

export function planLine(account: Account | null, licenseExpires: number | null): string {
  const end = account?.current_end ?? licenseExpires;
  if (account?.status === "billing_issue") return "Payment failed · update your card under Manage";
  const label = planLabel(account?.plan);
  if (account?.cancel_at_period_end) {
    const ends = end ? `ends ${formatPlanDate(end)}` : "ends at the end of this period";
    return label ? `${label} · ${ends}` : ends.charAt(0).toUpperCase() + ends.slice(1);
  }
  const head = label ?? PAWTROL_PRICES;
  return end ? `${head} · renews ${formatPlanDate(end)}` : head;
}

export function devicesLine(account: Account): string {
  return `${account.devices_count} of ${PAWTROL_MAX_DEVICES} Macs`;
}

export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}
