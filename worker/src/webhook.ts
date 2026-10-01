import {
  type Account,
  accountForCustomer,
  bindDevice,
  getAccount,
  isEntitled,
  putAccount,
  syncLicenses,
} from "./account";
import { secretsEqual } from "./crypto";
import type { Env } from "./env";
import { legacyError, legacyJson } from "./http";
import { getJson, nowSecs } from "./kv";
import { derivePlan, environmentAccepted, normalizeEnvironment } from "./revenuecat";
import { normalizeEmail } from "./validate";

export interface RcEvent {
  type?: string;
  id?: string;
  app_user_id?: string;
  environment?: string;
  store?: string;
  product_id?: string | null;
  new_product_id?: string | null;
  price?: number | null;
  event_timestamp_ms?: number;
  purchased_at_ms?: number | null;
  expiration_at_ms?: number | null;
  grace_period_expiration_at_ms?: number | null;
  cancel_reason?: string;
  subscriber_attributes?: Record<string, { value?: unknown } | undefined>;
  transferred_from?: unknown;
  transferred_to?: unknown;
}

export interface PendingCheckout {
  device_id: string;
  email: string;
}

export const pendingKey = (appUserId: string) => `pending:${appUserId}`;

const GRANTING = new Set(["INITIAL_PURCHASE", "RENEWAL", "UNCANCELLATION", "PRODUCT_CHANGE"]);
const HANDLED = new Set([...GRANTING, "CANCELLATION", "EXPIRATION", "BILLING_ISSUE", "TRANSFER"]);

const ok = (body: Record<string, unknown>) => legacyJson({ ok: true, ...body });
const secs = (ms: unknown) => (typeof ms === "number" ? Math.floor(ms / 1000) : null);
const isStale = (account: Account, ts: number | null) =>
  ts != null && account.last_event_at != null && ts < account.last_event_at;

// Refunds arrive as CANCELLATION. RevenueCat documents `CUSTOMER_SUPPORT` for refunded web
// subscriptions but only names RevenueCat Billing and Stripe Billing there, so `REFUND` and a
// negative `price` ("negative for refunds") also count, and a Paddle refund is never read as
// a plain cancellation.
function isRefund(event: RcEvent): boolean {
  const reason = event.cancel_reason;
  if (reason === "CUSTOMER_SUPPORT" || reason === "REFUND") return true;
  return reason !== "BILLING_ERROR" && typeof event.price === "number" && event.price < 0;
}

function applyEvent(env: Env, account: Account, event: RcEvent, type: string): void {
  const expires = secs(event.expiration_at_ms);
  account.environment = normalizeEnvironment(event.environment);
  // For RevenueCat Billing PRODUCT_CHANGE, `product_id` is the old product and `new_product_id` the new one.
  const plan = derivePlan(
    env,
    event.new_product_id ?? event.product_id,
    event.purchased_at_ms,
    event.expiration_at_ms
  );
  if (plan) account.plan = plan;
  if (GRANTING.has(type)) {
    account.status = "active";
    account.current_end = expires ?? account.current_end;
    account.grace_end = null;
    account.cancel_at_period_end = false;
    return;
  }
  switch (type) {
    case "CANCELLATION":
      if (isRefund(event)) {
        // A refund ends access immediately.
        account.status = "refunded";
        account.cancel_at_period_end = false;
      } else if (event.cancel_reason === "BILLING_ERROR") {
        // Sent alongside BILLING_ISSUE; the subscription is still being retried, not cancelled.
        account.status = "billing_issue";
        account.current_end = expires ?? account.current_end;
      } else {
        account.status = "cancelled";
        account.cancel_at_period_end = true;
        account.current_end = expires ?? account.current_end;
      }
      return;
    case "BILLING_ISSUE":
      account.status = "billing_issue";
      account.grace_end = secs(event.grace_period_expiration_at_ms);
      account.current_end = expires ?? account.current_end;
      return;
    case "EXPIRATION":
      account.status = "expired";
      account.grace_end = null;
      account.cancel_at_period_end = false;
      return;
  }
}

function outcome(account: Account, expires: number | null): Response {
  if (expires == null) return ok({ action: "deactivated" });
  if (account.status === "billing_issue") return ok({ action: "grace", expires });
  if (account.status === "cancelled") return ok({ action: "active_until_period_end", expires });
  return ok({ action: "activated", expires });
}

const strings = (v: unknown): string[] =>
  Array.isArray(v) ? v.filter((x): x is string => typeof x === "string" && x.length > 0) : [];

// The webhook carries no app_user_id for TRANSFER; the purchases move from the
// `transferred_from` customers to the first `transferred_to` one, and so does the account.
async function handleTransfer(env: Env, event: RcEvent, ts: number | null): Promise<Response> {
  const to = strings(event.transferred_to)[0];
  if (!to) return ok({ message: "Transfer without destination, skipped" });
  let moved = 0;
  for (const from of strings(event.transferred_from)) {
    const account = await accountForCustomer(env, from);
    if (!account || isStale(account, ts)) continue;
    account.app_user_id = to;
    if (ts != null) account.last_event_at = ts;
    await putAccount(env, account);
    await syncLicenses(env, account);
    moved++;
  }
  return ok({ action: "transferred", accounts: moved });
}

export async function handleRevenueCatWebhook(request: Request, env: Env): Promise<Response> {
  if (!(await secretsEqual(request.headers.get("Authorization") || "", env.REVENUECAT_WEBHOOK_AUTH))) {
    return legacyError("Unauthorized", 401);
  }

  let event: RcEvent;
  try {
    event = ((await request.json()) as { event?: RcEvent })?.event as RcEvent;
  } catch {
    return legacyError("Invalid JSON body", 400);
  }
  if (!event || typeof event !== "object") return ok({ message: "No event, skipped" });

  const type = typeof event.type === "string" ? event.type : "";
  if (!HANDLED.has(type)) return ok({ message: "Event ignored" });
  // Store-agnostic from here on: RC_BILLING and PADDLE (and any other store) take the same path.
  // Lifecycle events always carry `environment`; TRANSFER only sometimes, and its accounts are
  // already filtered by environment when they are looked up.
  const sandbox =
    type === "TRANSFER"
      ? event.environment != null && !environmentAccepted(env, event.environment)
      : !environmentAccepted(env, event.environment);
  if (sandbox) return ok({ message: "Sandbox event ignored" });

  // Retries reuse event_timestamp_ms, so ordering on it makes redelivery harmless.
  const ts = typeof event.event_timestamp_ms === "number" ? event.event_timestamp_ms : null;
  if (type === "TRANSFER") return handleTransfer(env, event, ts);

  const appUserId = typeof event.app_user_id === "string" && event.app_user_id ? event.app_user_id : null;
  if (!appUserId) return ok({ message: "No app_user_id, skipped" });

  const now = nowSecs();
  const known = await accountForCustomer(env, appUserId);
  const pending = await getJson<PendingCheckout>(env.LICENSES, pendingKey(appUserId));
  const email =
    known?.email ?? pending?.email ?? normalizeEmail(event.subscriber_attributes?.["$email"]?.value);
  if (!email) return ok({ message: "Unknown customer, skipped" });

  let account = known ?? (await getAccount(env, email));
  let firstSighting = false;

  if (account && account.app_user_id !== appUserId) {
    // Late events for a previous customer id must not override a newer live subscription.
    if (isEntitled(account, now) && !GRANTING.has(type)) {
      return ok({ message: "Superseded customer ignored" });
    }
    account.app_user_id = appUserId;
    account.current_end = null;
    account.grace_end = null;
    account.cancel_at_period_end = false;
    account.management_url = null;
    account.plan = null;
    account.last_event_at = null;
    firstSighting = true;
  }

  if (account && isStale(account, ts)) return ok({ message: "Stale event ignored" });

  if (!account) {
    account = {
      email,
      app_user_id: appUserId,
      status: "expired",
      current_end: null,
      grace_end: null,
      cancel_at_period_end: false,
      management_url: null,
      plan: null,
      environment: normalizeEnvironment(event.environment),
      devices: [],
      last_event_at: null,
    };
    firstSighting = true;
  }

  applyEvent(env, account, event, type);
  if (ts != null) account.last_event_at = Math.max(ts, account.last_event_at ?? 0);
  // Bind the purchasing Mac only once, so a device evicted later is not re-added on renewal.
  if (firstSighting && pending?.device_id) await bindDevice(env, account, pending.device_id, now);

  await putAccount(env, account);
  return outcome(account, await syncLicenses(env, account));
}
