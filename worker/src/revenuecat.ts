// Everything that depends on RevenueCat's URL formats and REST API lives here.
import { sha256Hex } from "./crypto";
import type { Env } from "./env";
import { DAY, nowSecs } from "./kv";

const API = "https://api.revenuecat.com/v2";
export const DEFAULT_ENTITLEMENT = "pawtrol";

// The entitlement identifier (lookup_key) in RevenueCat that unlocks Pawtrol.
export function entitlementId(env: Env): string {
  return (env.REVENUECAT_ENTITLEMENT || "").trim() || DEFAULT_ENTITLEMENT;
}

export class ProviderError extends Error {}

// The app_user_id travels in the checkout URL, and the device id is a bearer secret, so the
// first purchase uses a one-way derivation of it rather than the id itself.
export async function appUserIdForDevice(deviceId: string): Promise<string> {
  return `kyra-${await sha256Hex(`pawtrol-rc:${deviceId}`)}`;
}

export function sandboxAllowed(env: Env): boolean {
  return env.REVENUECAT_ALLOW_SANDBOX === "1";
}

export type Environment = "SANDBOX" | "PRODUCTION";

// Webhooks send `SANDBOX`/`PRODUCTION`, the REST API `sandbox`/`production`. Anything else,
// including a record written before environments were stored, counts as sandbox.
export function normalizeEnvironment(value: unknown): Environment {
  return typeof value === "string" && value.toUpperCase() === "PRODUCTION" ? "PRODUCTION" : "SANDBOX";
}

// Production always counts; sandbox only while REVENUECAT_ALLOW_SANDBOX=1, so sandbox licenses
// and accounts stop counting as soon as the flag is removed.
export function environmentAccepted(env: Env, value: unknown): boolean {
  return sandboxAllowed(env) || normalizeEnvironment(value) === "PRODUCTION";
}

export type Plan = "monthly" | "yearly";

// Whole tokens only, so an opaque id like `pri_01m2…` never reads as a period.
const YEARLY_ID = /(^|[^a-z0-9])(year|yearly|annual|annually|1y|p1y|12m|p12m)([^a-z0-9]|$)/;
const MONTHLY_ID = /(^|[^a-z0-9])(month|monthly|1m|p1m)([^a-z0-9]|$)/;

// Product ids decide when they name the period (e.g. `pawtrol_yearly`). Imported Paddle prices
// keep Paddle's opaque id (`pri_…`), so otherwise the period length decides
// (expiration minus purchase, as RevenueCat documents for the period duration).
export function derivePlan(productId: unknown, startMs: unknown, endMs: unknown): Plan | null {
  if (typeof productId === "string") {
    const id = productId.toLowerCase();
    if (YEARLY_ID.test(id)) return "yearly";
    if (MONTHLY_ID.test(id)) return "monthly";
  }
  if (typeof startMs !== "number" || typeof endMs !== "number" || endMs <= startMs) return null;
  const days = (endMs - startMs) / (DAY * 1000);
  if (days >= 26 && days <= 33) return "monthly";
  if (days >= 350 && days <= 380) return "yearly";
  return null;
}

// Web Purchase Link for an identified customer: `<link>/<app_user_id>?email=<email>`.
export function purchaseLink(env: Env, appUserId: string, email: string): string {
  const base = (env.REVENUECAT_WEB_PURCHASE_LINK || "").trim().replace(/\/+$/, "");
  if (!base.startsWith("https://")) throw new ProviderError("Web Purchase Link not configured");
  return `${base}/${encodeURIComponent(appUserId)}?email=${encodeURIComponent(email)}`;
}

interface RcSubscription {
  id: string;
  environment?: string;
  store?: string;
  product_id?: string | null;
  gives_access?: boolean;
  current_period_starts_at?: number | null;
  current_period_ends_at?: number | null;
  ends_at?: number | null;
  auto_renewal_status?: string;
  management_url?: string | null;
  entitlements?: { items?: { lookup_key?: string }[] };
}

export interface EntitlementState {
  active: boolean;
  expires: number | null;
  cancel_at_period_end: boolean;
  subscription_id: string | null;
  management_url: string | null;
  environment: Environment;
  plan: Plan | null;
}

async function call(env: Env, path: string): Promise<unknown | null> {
  if (!env.REVENUECAT_SECRET_API_KEY || !env.REVENUECAT_PROJECT_ID) {
    throw new ProviderError("RevenueCat API not configured");
  }
  let res: Response;
  try {
    res = await fetch(`${API}/projects/${encodeURIComponent(env.REVENUECAT_PROJECT_ID)}${path}`, {
      headers: { Authorization: `Bearer ${env.REVENUECAT_SECRET_API_KEY}`, Accept: "application/json" },
    });
  } catch {
    throw new ProviderError("RevenueCat unreachable");
  }
  if (res.status === 404) return null;
  if (!res.ok) throw new ProviderError(`RevenueCat responded ${res.status}`);
  try {
    return await res.json();
  } catch {
    throw new ProviderError("RevenueCat returned invalid JSON");
  }
}

async function pawtrolSubscription(env: Env, appUserId: string): Promise<RcSubscription | null> {
  const entitlement = entitlementId(env);
  const data = (await call(env, `/customers/${encodeURIComponent(appUserId)}/subscriptions`)) as
    | { items?: RcSubscription[] }
    | null;
  const subs = (data?.items ?? []).filter(
    (s) =>
      typeof s?.id === "string" &&
      environmentAccepted(env, s.environment) &&
      (s.entitlements?.items ?? []).some((e) => e?.lookup_key === entitlement)
  );
  const end = (s: RcSubscription) => s.ends_at ?? s.current_period_ends_at ?? 0;
  subs.sort((a, b) => Number(!!b.gives_access) - Number(!!a.gives_access) || end(b) - end(a));
  return subs[0] ?? null;
}

export async function fetchEntitlement(env: Env, appUserId: string): Promise<EntitlementState> {
  const sub = await pawtrolSubscription(env, appUserId);
  if (!sub) {
    return {
      active: false,
      expires: null,
      cancel_at_period_end: false,
      subscription_id: null,
      management_url: null,
      environment: "PRODUCTION",
      plan: null,
    };
  }
  const endMs = sub.ends_at ?? sub.current_period_ends_at;
  let expires = typeof endMs === "number" ? Math.floor(endMs / 1000) : null;
  // In a grace period the period end is already past; grant a short lease the next refresh replaces.
  if (sub.gives_access && (expires == null || expires <= nowSecs())) expires = nowSecs() + DAY;
  return {
    active: !!sub.gives_access,
    expires,
    cancel_at_period_end: sub.auto_renewal_status === "will_not_renew",
    subscription_id: sub.id,
    management_url: typeof sub.management_url === "string" ? sub.management_url : null,
    environment: normalizeEnvironment(sub.environment),
    plan: derivePlan(sub.product_id, sub.current_period_starts_at, sub.current_period_ends_at),
  };
}

export interface ManagementLinks {
  /** Short-lived authenticated link into the customer portal (cancel, change card, invoices). */
  url: string | null;
  /** The subscription's own `management_url`, when it has one. */
  stable: string | null;
}

const httpsOrNull = (v: unknown) => (typeof v === "string" && v.startsWith("https://") ? v : null);

// `authenticated_management_url` returns a single-use RevenueCat portal link for RevenueCat
// Billing and, for Paddle, a short-lived authenticated Paddle customer portal URL (when the
// Paddle API key has Customer portal sessions: Write). Its description still only names the
// Web Billing portal, so when it fails or returns nothing the subscription's own
// `management_url` is used, which RevenueCat also fills with a Paddle portal URL.
export async function managementLinks(env: Env, appUserId: string): Promise<ManagementLinks> {
  const sub = await pawtrolSubscription(env, appUserId);
  if (!sub) return { url: null, stable: null };
  const stable = httpsOrNull(sub.management_url);
  let authenticated: string | null = null;
  try {
    const data = (await call(
      env,
      `/subscriptions/${encodeURIComponent(sub.id)}/authenticated_management_url`
    )) as { management_url?: unknown } | null;
    authenticated = httpsOrNull(data?.management_url);
  } catch (err) {
    if (!stable) throw err;
    console.error(
      `authenticated management url failed, using management_url: ${err instanceof ProviderError ? err.message : "unexpected error"}`
    );
  }
  return { url: authenticated ?? stable, stable };
}
