import { accountForCustomer, accountForDevice, putAccount } from "./account";
import type { Env } from "./env";
import { legacyError, legacyJson } from "./http";
import { DAY, getJson, nowSecs, putJson, rateLimit } from "./kv";
import { mockEnabled } from "./mock";
import {
  type Environment,
  ProviderError,
  appUserIdForDevice,
  environmentAccepted,
  fetchEntitlement,
} from "./revenuecat";
import { isDeviceId } from "./validate";

export interface LicenseRecord {
  active: boolean;
  expires: number;
  app_user_id: string;
  /** Environment of the purchase; missing means sandbox. */
  environment?: Environment;
}

export const LICENSE_TTL_SECONDS = 35 * DAY;
const LICENSE_KV_GRACE_SECONDS = 5 * DAY;
const SCORE_LIMIT_WINDOW = 3600;
const SCORE_LIMIT_MAX = 10;
const REFRESH_INTERVAL_SECONDS = 600;

export const licenseKey = (deviceId: string) => `license:${deviceId}`;

export async function writeLicense(
  env: Env,
  deviceId: string,
  expires: number,
  appUserId: string,
  environment: Environment
): Promise<void> {
  const record: LicenseRecord = { active: true, expires, app_user_id: appUserId, environment };
  // Keep the record at least until the paid period ends (annual plans exceed 35 days).
  const ttl = Math.max(LICENSE_TTL_SECONDS, expires - nowSecs() + LICENSE_KV_GRACE_SECONDS);
  await putJson(env.LICENSES, licenseKey(deviceId), record, ttl);
}

// A license from an environment the worker does not accept right now counts as no license.
export async function readLicense(env: Env, deviceId: string): Promise<LicenseRecord | null> {
  const license = await getJson<LicenseRecord>(env.LICENSES, licenseKey(deviceId));
  return license && environmentAccepted(env, license.environment) ? license : null;
}

export async function deleteLicense(env: Env, deviceId: string): Promise<void> {
  await env.LICENSES.delete(licenseKey(deviceId));
}

export async function handleLicenseCheck(request: Request, env: Env): Promise<Response> {
  const deviceId = new URL(request.url).searchParams.get("device_id");
  if (!deviceId) return legacyError("Missing device_id", 400);

  const now = nowSecs();
  const license = await readLicense(env, deviceId);
  if (license?.active && license.expires >= now) {
    return legacyJson({ active: true, expires: license.expires });
  }

  const refreshed = await refreshFromRevenueCat(request, env, deviceId, now);
  if (refreshed != null) return legacyJson({ active: true, expires: refreshed });

  if (!license) return legacyJson({ active: false, expires: null });
  return legacyJson({ active: license.active && license.expires >= now, expires: license.expires });
}

// Webhooks are the source of truth; this covers a missed or delayed one (e.g. the app polling
// right after checkout) by asking RevenueCat directly, at most once per device per 10 minutes.
async function refreshFromRevenueCat(
  request: Request,
  env: Env,
  deviceId: string,
  now: number
): Promise<number | null> {
  if (!isDeviceId(deviceId) || !env.REVENUECAT_SECRET_API_KEY || mockEnabled(env, request)) return null;

  const throttleKey = `rcsync:${deviceId}`;
  if (await env.LICENSES.get(throttleKey)) return null;
  await putJson(env.LICENSES, throttleKey, now, REFRESH_INTERVAL_SECONDS);

  const account = await accountForDevice(env, deviceId);
  let appUserId = account?.app_user_id;
  if (!appUserId) {
    appUserId = await appUserIdForDevice(deviceId);
    // A known customer this device is not bound to (e.g. evicted by the device cap) gets nothing.
    if (await accountForCustomer(env, appUserId)) return null;
  }

  try {
    const state = await fetchEntitlement(env, appUserId);
    if (account) {
      let changed = false;
      if (state.management_url && state.management_url !== account.management_url) {
        account.management_url = state.management_url;
        changed = true;
      }
      if (state.plan && state.plan !== account.plan) {
        account.plan = state.plan;
        changed = true;
      }
      if (changed) await putAccount(env, account);
    }
    if (!state.active || state.expires == null || state.expires <= now) return null;
    await writeLicense(env, deviceId, state.expires, appUserId, state.environment);
    return state.expires;
  } catch (err) {
    console.error(`license refresh failed: ${err instanceof ProviderError ? err.message : "unexpected error"}`);
    return null;
  }
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

  const license = await readLicense(env, body.device_id);
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
