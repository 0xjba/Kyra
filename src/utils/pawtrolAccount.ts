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

export function planLine(account: Account | null, licenseExpires: number | null): string {
  const end = account?.current_end ?? licenseExpires;
  if (account?.status === "billing_issue") return "Payment failed · update your card under Manage";
  if (account?.cancel_at_period_end) return end ? `Ends ${formatPlanDate(end)}` : "Ends at the end of this period";
  const plan = account?.plan === "yearly" ? "Yearly plan" : account?.plan === "monthly" ? "Monthly plan" : "$0.99/month";
  return end ? `${plan} · renews ${formatPlanDate(end)}` : plan;
}

export function devicesLine(account: Account): string {
  return `${account.devices_count} of ${PAWTROL_MAX_DEVICES} Macs`;
}

export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}
