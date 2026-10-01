// Everything that depends on Paddle's API, payload formats and signature scheme lives here.
import type { SubStatus } from "./account";
import { hmacSha256Hex, sha256Hex, timingSafeEqualHex } from "./crypto";
import type { Env } from "./env";
import { DAY } from "./kv";

export class ProviderError extends Error {}

export type Plan = "monthly" | "yearly";
export const PLANS: readonly Plan[] = ["monthly", "yearly"];

/** Key in transaction/subscription `custom_data` that carries the derived customer reference. */
export const REF_KEY = "kyra_ref";
/** Access kept after a renewal payment fails, while Paddle retries it (dunning runs ~30 days). */
export const PAST_DUE_GRACE_SECONDS = 7 * DAY;
/** Accepted clock skew for `Paddle-Signature` timestamps. */
export const SIGNATURE_TOLERANCE_SECONDS = 300;

const API_VERSION = "1";

export type PaddleEnvironment = "sandbox" | "production";

export function paddleEnvironment(env: Env): PaddleEnvironment {
  return env.PADDLE_ENV === "production" ? "production" : "sandbox";
}

export function apiBase(env: Env): string {
  return paddleEnvironment(env) === "production" ? "https://api.paddle.com" : "https://sandbox-api.paddle.com";
}

// The reference travels through Paddle (custom_data, dashboard, logs) and the device id is a
// bearer secret, so a purchase is tagged with a one-way derivation of it rather than the id itself.
export async function refForDevice(deviceId: string): Promise<string> {
  return `kyra-${await sha256Hex(`pawtrol-ref:${deviceId}`)}`;
}

// Checkout pages must be https; plain http is only accepted on loopback, for sandbox testing
// against `wrangler dev`.
export function isCheckoutPageUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    const url = new URL(value);
    return url.protocol === "https:" || (url.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname));
  } catch {
    return false;
  }
}

export function priceIdForPlan(env: Env, plan: Plan): string {
  const id = (plan === "yearly" ? env.PADDLE_PRICE_ID_YEARLY : env.PADDLE_PRICE_ID_MONTHLY)?.trim();
  if (!id) throw new ProviderError(`price id for ${plan} not configured`);
  return id;
}

// ---- Webhook signatures ----------------------------------------------------------------

// `Paddle-Signature: ts=<unix>;h1=<hex hmac>[;h1=<hex hmac>]`, HMAC-SHA256 over `${ts}:${rawBody}`
// with the notification destination's secret. Several h1 values appear while a secret rotates.
export async function verifySignature(
  header: string | null,
  rawBody: string,
  secret: string,
  now: number
): Promise<boolean> {
  if (!header || !secret) return false;
  let ts: string | null = null;
  const signatures: string[] = [];
  for (const part of header.split(";")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    const key = part.slice(0, eq).trim();
    const value = part.slice(eq + 1).trim();
    if (key === "ts") ts = value;
    else if (key === "h1") signatures.push(value.toLowerCase());
  }
  if (!ts || !/^\d{1,12}$/.test(ts) || signatures.length === 0) return false;
  if (Math.abs(now - Number(ts)) > SIGNATURE_TOLERANCE_SECONDS) return false;

  const expected = await hmacSha256Hex(secret, `${ts}:${rawBody}`);
  let match = false;
  for (const sig of signatures) match = timingSafeEqualHex(expected, sig) || match;
  return match;
}

export async function signatureHeader(secret: string, rawBody: string, ts: number): Promise<string> {
  return `ts=${ts};h1=${await hmacSha256Hex(secret, `${ts}:${rawBody}`)}`;
}

// ---- Payload shapes (only the fields this worker reads) --------------------------------

interface Period {
  starts_at?: string;
  ends_at?: string;
}

interface PriceRef {
  id?: string;
  billing_cycle?: { interval?: string; frequency?: number } | null;
}

export interface PaddleSubscription {
  id?: string;
  status?: string;
  customer_id?: string;
  custom_data?: Record<string, unknown> | null;
  current_billing_period?: Period | null;
  billing_cycle?: { interval?: string; frequency?: number } | null;
  scheduled_change?: { action?: string; effective_at?: string } | null;
  items?: { price?: PriceRef }[];
}

export interface PaddleTransaction {
  id?: string;
  status?: string;
  origin?: string;
  customer_id?: string | null;
  subscription_id?: string | null;
  custom_data?: Record<string, unknown> | null;
  billing_period?: Period | null;
  items?: { price?: PriceRef }[];
  checkout?: { url?: string | null } | null;
}

export interface PaddleAdjustment {
  id?: string;
  action?: string;
  type?: string;
  status?: string;
  customer_id?: string;
  subscription_id?: string | null;
  transaction_id?: string;
}

/** Epoch seconds from an RFC 3339 string, or null. */
export function epochSecs(value: unknown): number | null {
  if (typeof value !== "string") return null;
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : null;
}

export function refFrom(customData: unknown): string | null {
  const ref = (customData as Record<string, unknown> | null | undefined)?.[REF_KEY];
  return typeof ref === "string" && /^kyra-[0-9a-f]{64}$/.test(ref) ? ref : null;
}

export function planFrom(
  env: Env,
  items: { price?: PriceRef }[] | undefined,
  cycle?: { interval?: string } | null
): Plan | null {
  const prices = (items ?? []).map((i) => i?.price).filter((p): p is PriceRef => !!p);
  for (const p of prices) {
    if (p.id && p.id === env.PADDLE_PRICE_ID_YEARLY) return "yearly";
    if (p.id && p.id === env.PADDLE_PRICE_ID_MONTHLY) return "monthly";
  }
  const interval = cycle?.interval ?? prices.find((p) => p.billing_cycle)?.billing_cycle?.interval;
  if (interval === "year") return "yearly";
  if (interval === "month") return "monthly";
  return null;
}

export interface SubscriptionState {
  status: SubStatus;
  current_end: number | null;
  grace_end: number | null;
  cancel_at_period_end: boolean;
  plan: Plan | null;
}

// Maps a subscription snapshot (webhook `data` or API entity) to the account state.
export function subscriptionState(env: Env, sub: PaddleSubscription): SubscriptionState {
  const plan = planFrom(env, sub.items, sub.billing_cycle);
  const periodEnd = epochSecs(sub.current_billing_period?.ends_at);
  const change = sub.scheduled_change;
  switch (sub.status) {
    case "active":
    case "trialing":
      if (change?.action === "cancel" || change?.action === "pause") {
        // Renewal is off; access continues until the change takes effect at the period end.
        return {
          status: "cancelled",
          current_end: epochSecs(change.effective_at) ?? periodEnd,
          grace_end: null,
          cancel_at_period_end: true,
          plan,
        };
      }
      return { status: "active", current_end: periodEnd, grace_end: null, cancel_at_period_end: false, plan };
    case "past_due": {
      // Paddle has already moved the billing period on to the unpaid one, so access is paid up to
      // its start; the grace period is counted from there so repeated events cannot extend it.
      const paidUntil = epochSecs(sub.current_billing_period?.starts_at) ?? periodEnd;
      return {
        status: "billing_issue",
        current_end: paidUntil,
        grace_end: paidUntil == null ? null : paidUntil + PAST_DUE_GRACE_SECONDS,
        cancel_at_period_end: false,
        plan,
      };
    }
    default:
      // canceled, paused, or anything unknown: no access.
      return { status: "expired", current_end: null, grace_end: null, cancel_at_period_end: false, plan };
  }
}

// ---- REST API --------------------------------------------------------------------------

interface ApiResult {
  ok: boolean;
  status: number;
  body: { data?: unknown; error?: { code?: string; detail?: string } } | null;
}

async function api(env: Env, method: string, path: string, payload?: unknown): Promise<ApiResult> {
  if (!env.PADDLE_API_KEY) throw new ProviderError("Paddle API not configured");
  let res: Response;
  try {
    res = await fetch(`${apiBase(env)}${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${env.PADDLE_API_KEY}`,
        "Paddle-Version": API_VERSION,
        Accept: "application/json",
        ...(payload !== undefined && { "Content-Type": "application/json" }),
      },
      body: payload === undefined ? undefined : JSON.stringify(payload),
    });
  } catch {
    throw new ProviderError("Paddle unreachable");
  }
  let body: ApiResult["body"] = null;
  try {
    body = (await res.json()) as ApiResult["body"];
  } catch {
    if (res.ok) throw new ProviderError("Paddle returned invalid JSON");
  }
  return { ok: res.ok, status: res.status, body };
}

function data<T>(result: ApiResult, what: string): T {
  if (!result.ok) throw new ProviderError(`Paddle ${what} responded ${result.status}`);
  const value = result.body?.data;
  if (!value || typeof value !== "object") throw new ProviderError(`Paddle ${what} returned no data`);
  return value as T;
}

const CUSTOMER_ID_RE = /ctm_[a-z\d]{26}/;

// Customer emails are unique in Paddle. Creating an existing one fails with 409
// `customer_already_exists` whose detail names the existing id; the email lookup is the fallback.
export async function findOrCreateCustomer(env: Env, email: string): Promise<string> {
  const created = await api(env, "POST", "/customers", { email });
  if (created.ok) {
    const id = data<{ id?: unknown }>(created, "create customer").id;
    if (typeof id === "string" && id) return id;
    throw new ProviderError("Paddle create customer returned no id");
  }
  if (created.status !== 409 || created.body?.error?.code !== "customer_already_exists") {
    throw new ProviderError(`Paddle create customer responded ${created.status}`);
  }
  const fromDetail = CUSTOMER_ID_RE.exec(created.body.error.detail ?? "")?.[0];
  if (fromDetail) return fromDetail;

  const list = await api(env, "GET", `/customers?email=${encodeURIComponent(email)}`);
  const customers = data<unknown[]>(list, "list customers");
  const match = Array.isArray(customers)
    ? (customers as { id?: unknown; email?: unknown }[]).find(
        (c) => typeof c?.id === "string" && typeof c.email === "string" && c.email.toLowerCase() === email
      )
    : undefined;
  if (!match) throw new ProviderError("Paddle customer conflict without a matching customer");
  return match.id as string;
}

export interface CreatedTransaction {
  id: string;
  checkout_url: string;
}

export async function createTransaction(
  env: Env,
  opts: { customerId: string; priceId: string; ref: string; checkoutUrl: string }
): Promise<CreatedTransaction> {
  const txn = data<PaddleTransaction>(
    await api(env, "POST", "/transactions", {
      items: [{ price_id: opts.priceId, quantity: 1 }],
      customer_id: opts.customerId,
      custom_data: { [REF_KEY]: opts.ref },
      collection_mode: "automatic",
      checkout: { url: opts.checkoutUrl },
    }),
    "create transaction"
  );
  const url = txn.checkout?.url;
  if (typeof txn.id !== "string" || !isCheckoutPageUrl(url)) {
    throw new ProviderError("Paddle transaction has no checkout URL");
  }
  return { id: txn.id, checkout_url: url };
}

export async function getSubscription(env: Env, id: string): Promise<PaddleSubscription | null> {
  const result = await api(env, "GET", `/subscriptions/${encodeURIComponent(id)}`);
  if (result.status === 404) return null;
  return data<PaddleSubscription>(result, "get subscription");
}

export async function getTransaction(env: Env, id: string): Promise<PaddleTransaction | null> {
  const result = await api(env, "GET", `/transactions/${encodeURIComponent(id)}`);
  if (result.status === 404) return null;
  return data<PaddleTransaction>(result, "get transaction");
}

// Authenticated, short-lived link into Paddle's customer portal: the subscription's page when
// Paddle returns one (cancel, resume, payment method), else the portal overview.
export async function portalLink(env: Env, customerId: string, subscriptionId: string | null): Promise<string | null> {
  const session = data<{
    urls?: { general?: { overview?: unknown }; subscriptions?: { id?: unknown; view_subscription?: unknown }[] };
  }>(
    await api(
      env,
      "POST",
      `/customers/${encodeURIComponent(customerId)}/portal-sessions`,
      subscriptionId ? { subscription_ids: [subscriptionId] } : {}
    ),
    "create portal session"
  );
  const https = (v: unknown) => (typeof v === "string" && v.startsWith("https://") ? v : null);
  const forSub = session.urls?.subscriptions?.find((s) => s?.id === subscriptionId);
  return https(forSub?.view_subscription) ?? https(session.urls?.general?.overview);
}
