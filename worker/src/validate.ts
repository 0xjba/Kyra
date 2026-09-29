const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const DEVICE_ID_RE = /^[0-9a-f]{64}$/;

export function normalizeEmail(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const email = value.trim().toLowerCase();
  return email.length <= 254 && EMAIL_RE.test(email) ? email : null;
}

export function isDeviceId(value: unknown): value is string {
  return typeof value === "string" && DEVICE_ID_RE.test(value);
}

export function maskEmail(email: string): string {
  const at = email.lastIndexOf("@");
  return `${email.slice(0, 1)}***${email.slice(at)}`;
}
