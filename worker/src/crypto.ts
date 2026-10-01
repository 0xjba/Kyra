const encoder = new TextEncoder();

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function randomHex(bytes: number): string {
  return bytesToHex(crypto.getRandomValues(new Uint8Array(bytes)));
}

export function randomCode(): string {
  // Rejection sampling keeps the six digits uniform.
  const limit = 2 ** 32 - (2 ** 32 % 1_000_000);
  const buf = new Uint32Array(1);
  do {
    crypto.getRandomValues(buf);
  } while (buf[0] >= limit);
  return String(buf[0] % 1_000_000).padStart(6, "0");
}

export async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(input));
  return bytesToHex(new Uint8Array(digest));
}

export function timingSafeEqualHex(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

// Hashing first makes the comparison independent of where (or whether) the lengths differ.
export async function secretsEqual(given: string, expected: string): Promise<boolean> {
  if (!expected) return false;
  return timingSafeEqualHex(await sha256Hex(given), await sha256Hex(expected));
}
