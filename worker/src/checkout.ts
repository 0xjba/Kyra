import { accountForDevice, isEntitled } from "./account";
import type { Env } from "./env";
import { clientIp, fail, invalidJson, json, readJson } from "./http";
import { DAY, nowSecs, putJson, rateLimit } from "./kv";
import { mockEnabled, mockPurchaseLink } from "./mock";
import { ProviderError, appUserIdForDevice, purchaseLink } from "./revenuecat";
import { isDeviceId, normalizeEmail } from "./validate";
import { type PendingCheckout, pendingKey } from "./webhook";

const CHECKOUT_LIMIT_PER_IP = 10;
const PENDING_TTL_SECONDS = 7 * DAY;

export async function handleCheckoutCreate(request: Request, env: Env): Promise<Response> {
  const body = await readJson(request);
  if (!body) return invalidJson();

  const email = normalizeEmail(body.email);
  if (!email) return fail(400, "invalid_email", "A valid email is required");
  if (!isDeviceId(body.device_id)) return fail(400, "invalid_device_id", "A valid device_id is required");
  const deviceId = body.device_id;

  if (!(await rateLimit(env.LICENSES, `rl:checkout:${clientIp(request)}`, CHECKOUT_LIMIT_PER_IP, 3600))) {
    return fail(429, "rate_limited", "Too many checkout attempts, try again later");
  }

  // A Mac already on an account resubscribes as the same RevenueCat customer.
  const account = await accountForDevice(env, deviceId);
  if (account && isEntitled(account, nowSecs())) {
    return fail(409, "already_active", "Pawtrol is already active on this Mac");
  }
  const appUserId = account?.app_user_id ?? (await appUserIdForDevice(deviceId));

  let shortUrl: string;
  if (mockEnabled(env, request)) {
    shortUrl = mockPurchaseLink(request, appUserId);
  } else {
    try {
      shortUrl = purchaseLink(env, appUserId, email);
    } catch (err) {
      console.error(`checkout failed: ${err instanceof ProviderError ? err.message : "unexpected error"}`);
      return fail(502, "payment_provider_error", "Could not start checkout, try again later");
    }
  }

  const pending: PendingCheckout = { device_id: deviceId, email };
  await putJson(env.LICENSES, pendingKey(appUserId), pending, PENDING_TTL_SECONDS);
  return json({ short_url: shortUrl, app_user_id: appUserId });
}
