import type { Env } from "./env";
import { legacyError, legacyJson } from "./http";
import { DAY, getJson, nowSecs, putJson, rateLimit } from "./kv";

export interface LicenseRecord {
  active: boolean;
  expires: number;
  subscription_id: string;
}

export const LICENSE_TTL_SECONDS = 35 * DAY;
const LICENSE_KV_GRACE_SECONDS = 5 * DAY;
const SCORE_LIMIT_WINDOW = 3600;
const SCORE_LIMIT_MAX = 10;

export const licenseKey = (deviceId: string) => `license:${deviceId}`;

export async function writeLicense(
  env: Env,
  deviceId: string,
  expires: number,
  subscriptionId: string
): Promise<void> {
  const record: LicenseRecord = { active: true, expires, subscription_id: subscriptionId };
  // Keep the record at least until the paid period ends (annual plans exceed 35 days).
  const ttl = Math.max(LICENSE_TTL_SECONDS, expires - nowSecs() + LICENSE_KV_GRACE_SECONDS);
  await putJson(env.LICENSES, licenseKey(deviceId), record, ttl);
}

export async function deleteLicense(env: Env, deviceId: string): Promise<void> {
  await env.LICENSES.delete(licenseKey(deviceId));
}

export async function handleLicenseCheck(request: Request, env: Env): Promise<Response> {
  const deviceId = new URL(request.url).searchParams.get("device_id");
  if (!deviceId) return legacyError("Missing device_id", 400);

  const license = await getJson<LicenseRecord>(env.LICENSES, licenseKey(deviceId));
  if (!license) return legacyJson({ active: false, expires: null });

  const active = license.active && license.expires >= nowSecs();
  return legacyJson({ active, expires: license.expires });
}

export async function handleJevScore(request: Request, env: Env): Promise<Response> {
  let body: { device_id?: string; questions?: string[] };
  try {
    body = await request.json();
  } catch {
    return legacyError("Invalid JSON body", 400);
  }
  if (!body?.device_id || !body.questions?.length) {
    return legacyError("Missing device_id or questions", 400);
  }

  const license = await getJson<LicenseRecord>(env.LICENSES, licenseKey(body.device_id));
  if (!license) return legacyError("No active license", 403);
  if (!license.active || license.expires < nowSecs()) return legacyError("License expired", 403);

  const allowed = await rateLimit(
    env.LICENSES,
    `ratelimit:${body.device_id}`,
    SCORE_LIMIT_MAX,
    SCORE_LIMIT_WINDOW
  );
  if (!allowed) return legacyError("Rate limit exceeded (10/hour)", 429);

  let upstream: Response;
  try {
    upstream = await fetch(env.JEV_API_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${env.JEV_API_KEY}`,
      },
      body: JSON.stringify({ questions: body.questions }),
    });
  } catch {
    return legacyError("Jev API unreachable", 502);
  }
  if (!upstream.ok) return legacyError(`Jev API error: ${upstream.status}`, 502);

  try {
    return legacyJson(await upstream.json());
  } catch {
    return legacyError("Jev API returned invalid JSON", 502);
  }
}
