import { accountForDevice, accountView } from "./account";
import type { Env } from "./env";
import { clientIp, fail, invalidJson, json, readJson } from "./http";
import { rateLimit } from "./kv";
import { mockEnabled, mockManageLink } from "./mock";
import { ProviderError, portalLink } from "./paddle";
import { isDeviceId } from "./validate";

const MANAGE_LIMIT_PER_IP = 20;

export async function handleAccount(request: Request, env: Env): Promise<Response> {
  const deviceId = new URL(request.url).searchParams.get("device_id");
  if (!isDeviceId(deviceId)) return fail(400, "invalid_device_id", "A valid device_id is required");

  const account = await accountForDevice(env, deviceId);
  if (!account) return fail(404, "not_found", "No account for this device");
  return json(accountView(account));
}

// Cancelling, resuming and payment method changes all happen in Paddle's customer portal.
export async function handleManage(request: Request, env: Env): Promise<Response> {
  const body = await readJson(request);
  if (!body) return invalidJson();
  if (!isDeviceId(body.device_id)) return fail(400, "invalid_device_id", "A valid device_id is required");

  if (!(await rateLimit(env.LICENSES, `rl:manage:${clientIp(request)}`, MANAGE_LIMIT_PER_IP, 3600))) {
    return fail(429, "rate_limited", "Too many requests, try again later");
  }

  const account = await accountForDevice(env, body.device_id);
  if (!account) return fail(404, "not_found", "No account for this device");

  if (mockEnabled(env, request)) return json({ url: mockManageLink(request, account.ref) });
  if (!account.customer_id) return fail(409, "no_active_subscription", "There is no subscription to manage");

  let url: string | null;
  try {
    url = await portalLink(env, account.customer_id, account.subscription_id);
  } catch (err) {
    console.error(`manage link failed: ${err instanceof ProviderError ? err.message : "unexpected error"}`);
    return fail(502, "payment_provider_error", "Could not open subscription management, try again later");
  }
  if (!url) return fail(409, "no_active_subscription", "There is no subscription to manage");
  return json({ url });
}

export async function handleCancelGone(): Promise<Response> {
  return fail(410, "use_management_url", "Cancel from the subscription management page");
}
