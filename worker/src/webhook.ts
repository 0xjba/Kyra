import {
  type Account,
  type SubStatus,
  accountForCustomer,
  accountForRef,
  accountForSubscription,
  bindDevice,
  entitlementExpiry,
  getAccount,
  isEntitled,
  putAccount,
  syncLicenses,
} from "./account";
import type { Env } from "./env";
import { legacyError, legacyJson } from "./http";
import { DAY, getJson, nowSecs, putJson } from "./kv";
import {
  type PaddleAdjustment,
  type PaddleSubscription,
  type PaddleTransaction,
  type Plan,
  ProviderError,
  epochSecs,
  getSubscription,
  pastDueWindow,
  planFrom,
  refFrom,
  subscriptionState,
  verifySignature,
} from "./paddle";

export interface PendingCheckout {
  device_id: string;
  email: string;
  customer_id: string | null;
  transaction_id: string | null;
  plan: Plan;
}

export interface PaddleEvent {
  event_id?: string;
  event_type?: string;
  occurred_at?: string;
  notification_id?: string;
  data?: unknown;
}

export const pendingKey = (ref: string) => `pending:${ref}`;
const eventKey = (eventId: string) => `pdlevt:${eventId}`;
const EVENT_SEEN_TTL_SECONDS = 7 * DAY;

const SUBSCRIPTION_EVENTS = new Set([
  "subscription.created",
  "subscription.activated",
  "subscription.updated",
  "subscription.canceled",
  "subscription.past_due",
  "subscription.paused",
  "subscription.resumed",
  "subscription.trialing",
]);
const ADJUSTMENT_EVENTS = new Set(["adjustment.created", "adjustment.updated"]);
// Money taken back in full: an approved full refund, or a chargeback (Paddle refunds the amount).
const REFUND_ACTIONS = new Set(["refund", "chargeback", "chargeback_warning"]);
// A dispute Paddle won (or a warning withdrawn) undoes the refund state that action caused.
const REVERSED_BY: Record<string, string> = {
  chargeback_reverse: "chargeback",
  chargeback_warning_reverse: "chargeback_warning",
};
// One-off charges and payment-method updates do not say anything about the paid period.
const IGNORED_TXN_ORIGINS = new Set(["subscription_charge", "subscription_payment_method_change"]);

const ok = (body: Record<string, unknown>) => legacyJson({ ok: true, ...body });
const isStale = (account: Account, ts: number | null) =>
  ts != null && account.last_event_at != null && ts < account.last_event_at;

interface Update {
  subscription_id: string;
  customer_id: string | null;
  ref: string | null;
  status: SubStatus;
  current_end: number | null;
  grace_end: number | null;
  /** undefined keeps the account's current value. */
  cancel_at_period_end?: boolean;
  /** Null when no item uses one of Pawtrol's configured prices: such events are ignored. */
  plan: Plan | null;
  /** A completed payment; only a payment lifts a refund on the same subscription. */
  payment: boolean;
}

function outcome(account: Account, expires: number | null): Response {
  if (expires == null) return ok({ action: "deactivated" });
  if (account.status === "billing_issue") return ok({ action: "grace", expires });
  if (account.status === "cancelled") return ok({ action: "active_until_period_end", expires });
  return ok({ action: "activated", expires });
}

function fromSubscription(env: Env, sub: PaddleSubscription): Update | null {
  if (typeof sub.id !== "string" || !sub.id) return null;
  return {
    subscription_id: sub.id,
    customer_id: typeof sub.customer_id === "string" ? sub.customer_id : null,
    ref: refFrom(sub.custom_data),
    ...subscriptionState(env, sub),
    payment: false,
  };
}

function fromTransaction(env: Env, txn: PaddleTransaction): Update | null {
  const end = epochSecs(txn.billing_period?.ends_at);
  if (typeof txn.subscription_id !== "string" || !txn.subscription_id || end == null) return null;
  if (txn.origin && IGNORED_TXN_ORIGINS.has(txn.origin)) return null;
  return {
    subscription_id: txn.subscription_id,
    customer_id: typeof txn.customer_id === "string" ? txn.customer_id : null,
    ref: refFrom(txn.custom_data),
    status: "active",
    current_end: end,
    grace_end: null,
    plan: planFrom(env, txn.items),
    payment: true,
  };
}

async function applyUpdate(env: Env, u: Update, ts: number | null): Promise<Response> {
  const now = nowSecs();
  const pending = u.ref ? await getJson<PendingCheckout>(env.LICENSES, pendingKey(u.ref)) : null;

  let account = await accountForSubscription(env, u.subscription_id);
  // Only purchases started by our checkout (tagged with a ref) may create or switch accounts.
  if (!account && u.ref) {
    account =
      (await accountForRef(env, u.ref)) ??
      (pending ? await getAccount(env, pending.email) : null) ??
      (u.customer_id ? await accountForCustomer(env, u.customer_id) : null);
  }
  const email = account?.email ?? pending?.email;
  if (!email) return ok({ message: "Unknown subscription, skipped" });

  // Ordering is checked against the account as it is, before any subscription switch.
  if (account && isStale(account, ts)) return ok({ message: "Stale event ignored" });

  let firstSighting = false;
  if (account && account.subscription_id !== u.subscription_id) {
    // Another subscription takes the account over only with a payment, or with a newer event that
    // also pays further ahead than what the account has now, so a late event for a superseded
    // subscription cannot hijack it.
    const newer = ts != null && (account.last_event_at == null || ts > account.last_event_at);
    const current = isEntitled(account, now) ? entitlementExpiry(account) : null;
    const offered = entitlementExpiry(u);
    const later = offered != null && (current == null || offered > current);
    if (!u.payment && !(newer && later)) return ok({ message: "Superseded subscription ignored" });
    account.subscription_id = u.subscription_id;
    if (u.ref) account.ref = u.ref;
    account.status = "expired";
    account.current_end = null;
    account.grace_end = null;
    account.cancel_at_period_end = false;
    account.management_url = null;
    delete account.refunded_by;
    firstSighting = true;
  }

  if (!account) {
    account = {
      email,
      ref: u.ref!,
      customer_id: null,
      subscription_id: u.subscription_id,
      plan: null,
      status: "expired",
      current_end: null,
      grace_end: null,
      cancel_at_period_end: false,
      management_url: null,
      devices: [],
      last_event_at: null,
    };
    firstSighting = true;
  }

  if (u.status === "billing_issue") {
    // Grace counts from the later of the unpaid period's start and the end already known to be paid.
    u = { ...u, ...pastDueWindow(u.current_end, account.current_end) };
  }

  const cancelAtPeriodEnd = u.cancel_at_period_end ?? account.cancel_at_period_end;
  if (account.status === "refunded" && !u.payment) {
    // A refunded subscription stays refunded (e.g. through the cancellation that follows) until paid again.
  } else {
    delete account.refunded_by;
    account.status = u.status === "active" && cancelAtPeriodEnd ? "cancelled" : u.status;
    account.current_end = u.current_end ?? account.current_end;
    account.grace_end = u.grace_end;
    account.cancel_at_period_end = u.status === "expired" ? false : cancelAtPeriodEnd;
  }
  if (u.customer_id) account.customer_id = u.customer_id;
  if (u.plan) account.plan = u.plan;
  if (ts != null) account.last_event_at = Math.max(ts, account.last_event_at ?? 0);
  // Bind the purchasing Mac only once, so a device evicted later is not re-added on renewal.
  if (firstSighting && pending?.device_id) await bindDevice(env, account, pending.device_id, now);

  await putAccount(env, account);
  return outcome(account, await syncLicenses(env, account));
}

// The refund-causing action an adjustment reverses: a `chargeback_reverse` /
// `chargeback_warning_reverse` adjustment, or the original chargeback once Paddle marks it `reversed`.
function reversedAction(adj: PaddleAdjustment): string | null {
  const action = adj.action ?? "";
  if (adj.type === "partial") return null;
  if (REVERSED_BY[action] && adj.status === "approved") return REVERSED_BY[action];
  if ((action === "chargeback" || action === "chargeback_warning") && adj.status === "reversed") return action;
  return null;
}

// Lifts a refund state after a won dispute: the subscription as Paddle has it now, else the paid
// period already recorded if it has not ended.
async function liftRefund(env: Env, account: Account, now: number): Promise<void> {
  let sub: PaddleSubscription | null = null;
  try {
    if (account.subscription_id) sub = await getSubscription(env, account.subscription_id);
  } catch (err) {
    console.error(`reversal lookup failed: ${err instanceof ProviderError ? err.message : "unexpected error"}`);
  }
  delete account.refunded_by;
  const state = sub ? subscriptionState(env, sub, account.current_end) : null;
  if (state?.plan) {
    account.status = state.status;
    account.current_end = state.current_end ?? account.current_end;
    account.grace_end = state.grace_end;
    account.cancel_at_period_end = state.cancel_at_period_end;
    account.plan = state.plan;
  } else if (!state && account.current_end != null && account.current_end > now) {
    account.status = "active";
    account.grace_end = null;
  } else {
    account.status = "expired";
    account.grace_end = null;
    account.cancel_at_period_end = false;
  }
}

async function applyAdjustment(env: Env, adj: PaddleAdjustment, ts: number | null): Promise<Response> {
  const reverses = reversedAction(adj);
  const refunds = REFUND_ACTIONS.has(adj.action ?? "") && adj.type === "full" && adj.status === "approved";
  if (!refunds && !reverses) return ok({ message: "Adjustment ignored" });
  if (typeof adj.subscription_id !== "string" || !adj.subscription_id) {
    return ok({ message: "Adjustment without subscription, skipped" });
  }
  const account = await accountForSubscription(env, adj.subscription_id);
  if (!account) return ok({ message: "Unknown subscription, skipped" });
  if (isStale(account, ts)) return ok({ message: "Stale event ignored" });

  if (reverses) {
    // Only undoes the refund state that this kind of dispute caused, never a genuine refund.
    if (account.status !== "refunded" || account.refunded_by !== reverses) {
      return ok({ message: "Reversal without matching refund, skipped" });
    }
    await liftRefund(env, account, nowSecs());
  } else {
    // A full refund ends access immediately.
    account.status = "refunded";
    account.refunded_by = adj.action;
    account.grace_end = null;
    account.cancel_at_period_end = false;
  }
  if (ts != null) account.last_event_at = Math.max(ts, account.last_event_at ?? 0);
  await putAccount(env, account);
  return outcome(account, await syncLicenses(env, account));
}

export async function handlePaddleWebhook(request: Request, env: Env): Promise<Response> {
  // The signature covers the exact bytes, so read the raw body before parsing anything.
  const raw = await request.text();
  if (!(await verifySignature(request.headers.get("Paddle-Signature"), raw, env.PADDLE_WEBHOOK_SECRET, nowSecs()))) {
    return legacyError("Unauthorized", 401);
  }

  let event: PaddleEvent;
  try {
    event = JSON.parse(raw) as PaddleEvent;
  } catch {
    return legacyError("Invalid JSON body", 400);
  }
  if (!event || typeof event !== "object" || !event.data || typeof event.data !== "object") {
    return ok({ message: "No event, skipped" });
  }

  const type = typeof event.event_type === "string" ? event.event_type : "";
  const handled = SUBSCRIPTION_EVENTS.has(type) || ADJUSTMENT_EVENTS.has(type) || type === "transaction.completed";
  if (!handled) return ok({ message: "Event ignored" });

  const eventId = typeof event.event_id === "string" && event.event_id ? event.event_id : null;
  if (eventId && (await env.LICENSES.get(eventKey(eventId)))) return ok({ message: "Duplicate event ignored" });

  // Deliveries can arrive out of order; occurred_at orders them (epoch ms).
  const occurred = typeof event.occurred_at === "string" ? Date.parse(event.occurred_at) : NaN;
  const ts = Number.isFinite(occurred) ? occurred : null;

  let response: Response;
  if (ADJUSTMENT_EVENTS.has(type)) {
    response = await applyAdjustment(env, event.data as PaddleAdjustment, ts);
  } else {
    const update =
      type === "transaction.completed"
        ? fromTransaction(env, event.data as PaddleTransaction)
        : fromSubscription(env, event.data as PaddleSubscription);
    response = !update
      ? ok({ message: "Not a subscription event, skipped" })
      : !update.plan
        ? ok({ message: "Unknown price, ignored" })
        : await applyUpdate(env, update, ts);
  }

  if (eventId) await putJson(env.LICENSES, eventKey(eventId), 1, EVENT_SEEN_TTL_SECONDS);
  return response;
}
