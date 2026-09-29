import { LIVE_STATUSES, type SubStatus, accountForDevice, accountView, putAccount, syncLicenses } from "./account";
import type { Env } from "./env";
import { fail, invalidJson, json, readJson } from "./http";
import { mockEnabled } from "./mock";
import { ProviderError, type RazorpaySubscription, cancelSubscriptionAtCycleEnd } from "./razorpay";
import { isDeviceId } from "./validate";

export async function handleAccount(request: Request, env: Env): Promise<Response> {
  const deviceId = new URL(request.url).searchParams.get("device_id");
  if (!isDeviceId(deviceId)) return fail(400, "invalid_device_id", "A valid device_id is required");

  const account = await accountForDevice(env, deviceId);
  if (!account) return fail(404, "not_found", "No account for this device");
  return json(accountView(account));
}

export async function handleCancel(request: Request, env: Env): Promise<Response> {
  const body = await readJson(request);
  if (!body) return invalidJson();
  if (!isDeviceId(body.device_id)) return fail(400, "invalid_device_id", "A valid device_id is required");

  const account = await accountForDevice(env, body.device_id);
  if (!account) return fail(404, "not_found", "No account for this device");
  if (account.cancel_at_period_end) return json(accountView(account));
  if (!LIVE_STATUSES.has(account.status)) {
    return fail(409, "no_active_subscription", "There is no active subscription to cancel");
  }

  let sub: RazorpaySubscription;
  if (mockEnabled(env, request)) {
    sub = { id: account.subscription_id, status: account.status };
  } else {
    try {
      sub = await cancelSubscriptionAtCycleEnd(env, account.subscription_id);
    } catch (err) {
      console.error(`cancel failed: ${err instanceof ProviderError ? err.message : "unexpected error"}`);
      return fail(502, "payment_provider_error", "Could not cancel right now, try again later");
    }
  }

  account.cancel_at_period_end = true;
  if (sub.status === "cancelled" || LIVE_STATUSES.has(sub.status as SubStatus)) {
    account.status = sub.status as SubStatus;
  }
  if (typeof sub.current_end === "number") account.current_end = sub.current_end;
  await putAccount(env, account);
  await syncLicenses(env, account);
  return json(accountView(account));
}
