import { accountForDevice, isEntitled } from "./account";
import type { Env } from "./env";
import { clientIp, fail, invalidJson, json, readJson } from "./http";
import { DAY, nowSecs, putJson, rateLimit } from "./kv";
import { mockEnabled, mockPurchaseLink } from "./mock";
import {
  PLANS,
  type Plan,
  ProviderError,
  createTransaction,
  isCheckoutPageUrl,
  findOrCreateCustomer,
  priceIdForPlan,
  refForDevice,
} from "./paddle";
import { isDeviceId, normalizeEmail } from "./validate";
import { type PendingCheckout, pendingKey } from "./webhook";

const CHECKOUT_LIMIT_PER_IP = 10;
const PENDING_TTL_SECONDS = 7 * DAY;

// The page Paddle's checkout link points at: an approved-domain page when configured,
// else this worker's own /pay page.
function checkoutPage(request: Request, env: Env): string {
  const configured = (env.PADDLE_CHECKOUT_URL || "").trim();
  if (configured) {
    if (!isCheckoutPageUrl(configured)) throw new ProviderError("PADDLE_CHECKOUT_URL must be https");
    return configured;
  }
  return `${new URL(request.url).origin}/pay`;
}

export async function handleCheckoutCreate(request: Request, env: Env): Promise<Response> {
  const body = await readJson(request);
  if (!body) return invalidJson();

  const email = normalizeEmail(body.email);
  if (!email) return fail(400, "invalid_email", "A valid email is required");
  if (!isDeviceId(body.device_id)) return fail(400, "invalid_device_id", "A valid device_id is required");
  const deviceId = body.device_id;
  // Older app versions send no plan; they get the monthly one.
  if (body.plan != null && !PLANS.includes(body.plan as Plan)) {
    return fail(400, "invalid_plan", "plan must be \"monthly\" or \"yearly\"");
  }
  const plan: Plan = (body.plan as Plan | undefined) ?? "monthly";

  if (!(await rateLimit(env.LICENSES, `rl:checkout:${clientIp(request)}`, CHECKOUT_LIMIT_PER_IP, 3600))) {
    return fail(429, "rate_limited", "Too many checkout attempts, try again later");
  }

  // A Mac already on an account resubscribes under the account's reference.
  const account = await accountForDevice(env, deviceId);
  if (account && isEntitled(account, nowSecs())) {
    return fail(409, "already_active", "Pawtrol is already active on this Mac");
  }
  const ref = account?.ref ?? (await refForDevice(deviceId));

  let shortUrl: string;
  let customerId: string | null = null;
  let transactionId: string | null = null;
  if (mockEnabled(env, request)) {
    shortUrl = mockPurchaseLink(request, ref, plan);
  } else {
    try {
      const priceId = priceIdForPlan(env, plan);
      const page = checkoutPage(request, env);
      customerId =
        account?.email === email && account.customer_id ? account.customer_id : await findOrCreateCustomer(env, email);
      const txn = await createTransaction(env, { customerId, priceId, ref, checkoutUrl: page });
      shortUrl = txn.checkout_url;
      transactionId = txn.id;
    } catch (err) {
      console.error(`checkout failed: ${err instanceof ProviderError ? err.message : "unexpected error"}`);
      return fail(502, "payment_provider_error", "Could not start checkout, try again later");
    }
  }

  const pending: PendingCheckout = {
    device_id: deviceId,
    email,
    customer_id: customerId,
    transaction_id: transactionId,
    plan,
  };
  await putJson(env.LICENSES, pendingKey(ref), pending, PENDING_TTL_SECONDS);
  return json({ short_url: shortUrl, app_user_id: ref });
}
