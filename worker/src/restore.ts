import { bindDevice, entitlementExpiry, getAccount, isEntitled, putAccount } from "./account";
import { randomCode, randomHex, sha256Hex, timingSafeEqualHex } from "./crypto";
import type { Ctx, Env } from "./env";
import { clientIp, fail, invalidJson, json, readJson } from "./http";
import { getJson, nowSecs, putJson, rateLimit } from "./kv";
import { writeLicense } from "./license";
import { sendRestoreCode } from "./mail";
import { mockEnabled } from "./mock";
import { normalizeEnvironment } from "./revenuecat";
import { isDeviceId, normalizeEmail } from "./validate";

const CODE_TTL_SECONDS = 600;
const MAX_ATTEMPTS = 5;
const SENDS_PER_EMAIL = 3;
const STARTS_PER_IP = 20;

interface RestoreCode {
  hash: string;
  salt: string;
  expires: number;
  attempts: number;
}

const restoreKey = (email: string) => `restore:${email}`;
const hashCode = (salt: string, email: string, code: string) => sha256Hex(`${salt}:${email}:${code}`);

export async function handleRestoreStart(request: Request, env: Env, ctx?: Ctx): Promise<Response> {
  const body = await readJson(request);
  if (!body) return invalidJson();
  const email = normalizeEmail(body.email);
  if (!email) return fail(400, "invalid_email", "A valid email is required");

  if (!(await rateLimit(env.LICENSES, `rl:restore-ip:${clientIp(request)}`, STARTS_PER_IP, 3600))) {
    return fail(429, "rate_limited", "Too many requests, try again later");
  }
  // Counted for every address, known or not, so a 429 says nothing about accounts.
  if (!(await rateLimit(env.LICENSES, `rl:restore-email:${email}`, SENDS_PER_EMAIL, 3600))) {
    return fail(429, "rate_limited", "Too many codes requested for this email, try again later");
  }

  const now = nowSecs();
  const account = await getAccount(env, email);
  if (account && isEntitled(account, now)) {
    const code = randomCode();
    const salt = randomHex(16);
    const record: RestoreCode = {
      hash: await hashCode(salt, email, code),
      salt,
      expires: now + CODE_TTL_SECONDS,
      attempts: 0,
    };
    await putJson(env.LICENSES, restoreKey(email), record, CODE_TTL_SECONDS);

    // Sending after the response keeps timing the same for known and unknown emails.
    const send = sendRestoreCode(env, email, code, mockEnabled(env, request)).catch((err) =>
      console.error(`restore email failed: ${err instanceof Error ? err.message : "unknown"}`)
    );
    if (ctx) ctx.waitUntil(send);
    else await send;
  }

  return json({ ok: true });
}

export async function handleRestoreVerify(request: Request, env: Env): Promise<Response> {
  const body = await readJson(request);
  if (!body) return invalidJson();
  const email = normalizeEmail(body.email);
  if (!email) return fail(400, "invalid_email", "A valid email is required");
  if (!isDeviceId(body.device_id)) return fail(400, "invalid_device_id", "A valid device_id is required");
  const deviceId = body.device_id;
  const code = typeof body.code === "string" ? body.code.trim() : "";
  if (!/^\d{6}$/.test(code)) return fail(400, "invalid_code", "Invalid or expired code");

  const key = restoreKey(email);
  const record = await getJson<RestoreCode>(env.LICENSES, key);
  const now = nowSecs();
  if (!record) return fail(400, "invalid_code", "Invalid or expired code");
  if (record.expires <= now) {
    await env.LICENSES.delete(key);
    return fail(400, "code_expired", "This code has expired, request a new one");
  }
  if (record.attempts >= MAX_ATTEMPTS) {
    await env.LICENSES.delete(key);
    return fail(429, "too_many_attempts", "Too many attempts, request a new code");
  }

  if (!timingSafeEqualHex(await hashCode(record.salt, email, code), record.hash)) {
    record.attempts++;
    if (record.attempts >= MAX_ATTEMPTS) {
      await env.LICENSES.delete(key);
      return fail(429, "too_many_attempts", "Too many attempts, request a new code");
    }
    await putJson(env.LICENSES, key, record, record.expires - now);
    return fail(400, "invalid_code", "Invalid or expired code");
  }

  await env.LICENSES.delete(key);
  const account = await getAccount(env, email);
  if (!account || !isEntitled(account, now)) {
    return fail(403, "subscription_inactive", "This subscription is no longer active");
  }

  await bindDevice(env, account, deviceId, now);
  await putAccount(env, account);
  const expires = entitlementExpiry(account)!;
  await writeLicense(env, deviceId, expires, account.app_user_id, normalizeEnvironment(account.environment));
  return json({ active: true, expires, app_user_id: account.app_user_id });
}
