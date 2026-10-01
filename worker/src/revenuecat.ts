// Everything that depends on RevenueCat's URL formats and REST API lives here.
import { sha256Hex } from "./crypto";
import type { Env } from "./env";
import { DAY, nowSecs } from "./kv";

const API = "https://api.revenuecat.com/v2";
export const ENTITLEMENT = "pawtrol";

export class ProviderError extends Error {}

// The app_user_id travels in the checkout URL, and the device id is a bearer secret, so the
// first purchase uses a one-way derivation of it rather than the id itself.
export async function appUserIdForDevice(deviceId: string): Promise<string> {
  return `kyra-${await sha256Hex(`pawtrol-rc:${deviceId}`)}`;
}

export function sandboxAllowed(env: Env): boolean {
  return env.REVENUECAT_ALLOW_SANDBOX === "1";
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
  gives_access?: boolean;
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
  const data = (await call(env, `/customers/${encodeURIComponent(appUserId)}/subscriptions`)) as
    | { items?: RcSubscription[] }
    | null;
  const subs = (data?.items ?? []).filter(
    (s) =>
      typeof s?.id === "string" &&
      (sandboxAllowed(env) || s.environment !== "sandbox") &&
      (s.entitlements?.items ?? []).some((e) => e?.lookup_key === ENTITLEMENT)
  );
  const end = (s: RcSubscription) => s.ends_at ?? s.current_period_ends_at ?? 0;
  subs.sort((a, b) => Number(!!b.gives_access) - Number(!!a.gives_access) || end(b) - end(a));
  return subs[0] ?? null;
}

export async function fetchEntitlement(env: Env, appUserId: string): Promise<EntitlementState> {
  const sub = await pawtrolSubscription(env, appUserId);
  if (!sub) {
    return { active: false, expires: null, cancel_at_period_end: false, subscription_id: null, management_url: null };
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
  };
}

export interface ManagementLinks {
  /** Single-use magic link into the customer portal (cancel, change card, invoices). */
  url: string | null;
  /** RevenueCat's long-lived management URL for the subscription, when it has one. */
  stable: string | null;
}

const httpsOrNull = (v: unknown) => (typeof v === "string" && v.startsWith("https://") ? v : null);

export async function managementLinks(env: Env, appUserId: string): Promise<ManagementLinks> {
  const sub = await pawtrolSubscription(env, appUserId);
  if (!sub) return { url: null, stable: null };
  const stable = httpsOrNull(sub.management_url);
  const data = (await call(
    env,
    `/subscriptions/${encodeURIComponent(sub.id)}/authenticated_management_url`
  )) as { management_url?: unknown } | null;
  return { url: httpsOrNull(data?.management_url) ?? stable, stable };
}
