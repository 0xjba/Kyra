import { accountForDevice, accountForRef, getAccount } from "./account";
import type { Env } from "./env";
import { legacyError, legacyJson } from "./http";
import { DAY, getJson, nowSecs, putJson, rateLimit } from "./kv";
import { mockEnabled } from "./mock";
import {
  type PaddleEnvironment,
  ProviderError,
  type PaddleSubscription,
  getSubscription,
  getTransaction,
  paddleEnvironment,
  refForDevice,
  refFrom,
  sameEnvironment,
  subscriptionState,
} from "./paddle";
import { isDeviceId } from "./validate";
import { type PendingCheckout, pendingKey } from "./webhook";

export interface LicenseRecord {
  active: boolean;
  expires: number;
  ref: string;
  /** Paddle environment the license was granted in; missing means sandbox. */
  paddle_env?: PaddleEnvironment;
}

export const LICENSE_TTL_SECONDS = 35 * DAY;
const LICENSE_KV_GRACE_SECONDS = 5 * DAY;
const SCORE_LIMIT_WINDOW = 3600;
const SCORE_LIMIT_MAX = 10;
const REFRESH_INTERVAL_SECONDS = 600;

export const licenseKey = (deviceId: string) => `license:${deviceId}`;

export async function writeLicense(env: Env, deviceId: string, expires: number, ref: string): Promise<void> {
  const record: LicenseRecord = { active: true, expires, ref, paddle_env: paddleEnvironment(env) };
  // Keep the record at least until the paid period ends (yearly plans exceed 35 days).
  const ttl = Math.max(LICENSE_TTL_SECONDS, expires - nowSecs() + LICENSE_KV_GRACE_SECONDS);
  await putJson(env.LICENSES, licenseKey(deviceId), record, ttl);
}

// A license granted in the other Paddle environment (sandbox vs production) does not exist here.
export async function readLicense(env: Env, deviceId: string): Promise<LicenseRecord | null> {
  const license = await getJson<LicenseRecord>(env.LICENSES, licenseKey(deviceId));
  return license && sameEnvironment(env, license) ? license : null;
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

  const refreshed = await refreshFromPaddle(request, env, deviceId, now);
  if (refreshed != null) return legacyJson({ active: true, expires: refreshed });

  if (!license) return legacyJson({ active: false, expires: null });
  return legacyJson({ active: license.active && license.expires >= now, expires: license.expires });
}

function accessUntil(env: Env, sub: PaddleSubscription | null, now: number, knownEnd?: number | null): number | null {
  if (!sub) return null;
  const state = subscriptionState(env, sub, knownEnd);
  // Only Pawtrol's own prices grant access.
  if (state.plan == null) return null;
  const expires = state.status === "billing_issue" ? state.grace_end : state.current_end;
  return state.status !== "expired" && expires != null && expires > now ? expires : null;
}

// Webhooks are the source of truth; this covers a missed or delayed one (e.g. the app polling
// right after checkout) by asking Paddle directly, at most once per device per 10 minutes.
async function refreshFromPaddle(request: Request, env: Env, deviceId: string, now: number): Promise<number | null> {
  if (!isDeviceId(deviceId) || !env.PADDLE_API_KEY || mockEnabled(env, request)) return null;

  const throttleKey = `pdlsync:${deviceId}`;
  if (await env.LICENSES.get(throttleKey)) return null;
  await putJson(env.LICENSES, throttleKey, now, REFRESH_INTERVAL_SECONDS);

  const account = await accountForDevice(env, deviceId);
  // Paddle may keep a subscription active after a full refund; only a new payment (webhook) lifts it.
  if (account?.status === "refunded") return null;
  let ref = account?.ref;
  if (!ref) {
    ref = await refForDevice(deviceId);
    // A known customer this device is not bound to (e.g. evicted by the device cap) gets nothing.
    if (await accountForRef(env, ref)) return null;
  }

  try {
    let expires = account?.subscription_id
      ? accessUntil(env, await getSubscription(env, account.subscription_id), now, account.current_end)
      : null;

    // A checkout this very Mac started whose webhooks have not arrived yet, unless its account is refunded.
    const pending = await getJson<PendingCheckout>(env.LICENSES, pendingKey(ref));
    if (
      expires == null &&
      pending?.device_id === deviceId &&
      pending.transaction_id &&
      (await getAccount(env, pending.email))?.status !== "refunded"
    ) {
      const txn = await getTransaction(env, pending.transaction_id);
      const subId = txn?.subscription_id;
      if (txn && refFrom(txn.custom_data) === ref && typeof subId === "string" && subId !== account?.subscription_id) {
        expires = accessUntil(env, await getSubscription(env, subId), now);
      }
    }

    if (expires == null) return null;
    await writeLicense(env, deviceId, expires, ref);
    return expires;
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
