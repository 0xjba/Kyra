import type { Env } from "./env";
import { getJson, putJson } from "./kv";
import { deleteLicense, writeLicense } from "./license";
import type { Plan } from "./paddle";
import { maskEmail } from "./validate";

export type SubStatus = "active" | "cancelled" | "billing_issue" | "expired" | "refunded";

export interface Device {
  device_id: string;
  bound_at: number;
}

export interface Account {
  email: string;
  /** Derived customer reference (`kyra-…`), carried in Paddle `custom_data`. Returned to the app as `app_user_id`. */
  ref: string;
  /** Paddle customer (`ctm_…`); emails are unique per Paddle customer. */
  customer_id: string | null;
  /** Paddle subscription (`sub_…`) currently backing the account. */
  subscription_id: string | null;
  plan: Plan | null;
  status: SubStatus;
  /** Epoch seconds the current paid period ends. */
  current_end: number | null;
  /** Epoch seconds a billing-issue grace period ends, when one is configured. */
  grace_end: number | null;
  cancel_at_period_end: boolean;
  management_url: string | null;
  devices: Device[];
  /** Paddle `occurred_at` (epoch ms) of the newest applied event. */
  last_event_at: number | null;
}

export type SubState = Pick<Account, "status" | "current_end" | "grace_end">;

export const MAX_DEVICES = 3;

const accountKey = (email: string) => `account:${email}`;
const deviceKey = (deviceId: string) => `device:${deviceId}`;
export const refKey = (ref: string) => `pdlref:${ref}`;
export const subscriptionKey = (subscriptionId: string) => `pdlsub:${subscriptionId}`;
export const customerKey = (customerId: string) => `pdlcus:${customerId}`;

export function entitlementExpiry(s: SubState): number | null {
  switch (s.status) {
    case "active":
    case "cancelled":
      return s.current_end;
    case "billing_issue":
      return s.grace_end ?? s.current_end;
    default:
      return null;
  }
}

export function isEntitled(s: SubState, now: number): boolean {
  const expires = entitlementExpiry(s);
  return expires != null && expires > now;
}

export function getAccount(env: Env, email: string): Promise<Account | null> {
  return getJson<Account>(env.LICENSES, accountKey(email));
}

export async function putAccount(env: Env, account: Account): Promise<void> {
  await putJson(env.LICENSES, accountKey(account.email), account);
  await env.LICENSES.put(refKey(account.ref), account.email);
  if (account.subscription_id) await env.LICENSES.put(subscriptionKey(account.subscription_id), account.email);
  if (account.customer_id) await env.LICENSES.put(customerKey(account.customer_id), account.email);
}

// Reverse indexes are only hints: the account must still hold the id, so stale entries are harmless.
async function accountVia(
  env: Env,
  key: string,
  matches: (account: Account) => boolean
): Promise<Account | null> {
  const email = await env.LICENSES.get(key);
  if (!email) return null;
  const account = await getAccount(env, email);
  return account && matches(account) ? account : null;
}

export const accountForRef = (env: Env, ref: string) => accountVia(env, refKey(ref), (a) => a.ref === ref);

export const accountForSubscription = (env: Env, subscriptionId: string) =>
  accountVia(env, subscriptionKey(subscriptionId), (a) => a.subscription_id === subscriptionId);

export const accountForCustomer = (env: Env, customerId: string) =>
  accountVia(env, customerKey(customerId), (a) => a.customer_id === customerId);

export async function accountForDevice(env: Env, deviceId: string): Promise<Account | null> {
  const email = await env.LICENSES.get(deviceKey(deviceId));
  if (!email) return null;
  const account = await getAccount(env, email);
  return account?.devices.some((d) => d.device_id === deviceId) ? account : null;
}

// Mutates `account`; the caller persists it.
export async function bindDevice(env: Env, account: Account, deviceId: string, now: number): Promise<void> {
  if (account.devices.some((d) => d.device_id === deviceId)) return;

  const previousEmail = await env.LICENSES.get(deviceKey(deviceId));
  if (previousEmail && previousEmail !== account.email) {
    const previous = await getAccount(env, previousEmail);
    if (previous) {
      previous.devices = previous.devices.filter((d) => d.device_id !== deviceId);
      await putAccount(env, previous);
    }
  }

  account.devices.sort((a, b) => a.bound_at - b.bound_at);
  while (account.devices.length >= MAX_DEVICES) {
    const evicted = account.devices.shift()!;
    await deleteLicense(env, evicted.device_id);
    if ((await env.LICENSES.get(deviceKey(evicted.device_id))) === account.email) {
      await env.LICENSES.delete(deviceKey(evicted.device_id));
    }
  }

  account.devices.push({ device_id: deviceId, bound_at: now });
  await env.LICENSES.put(deviceKey(deviceId), account.email);
}

export async function syncLicenses(env: Env, account: Account): Promise<number | null> {
  const expires = entitlementExpiry(account);
  for (const { device_id } of account.devices) {
    if (expires == null) await deleteLicense(env, device_id);
    else await writeLicense(env, device_id, expires, account.ref);
  }
  return expires;
}

export function accountView(account: Account) {
  return {
    email: maskEmail(account.email),
    status: account.status,
    plan: account.plan ?? null,
    current_end: account.current_end,
    cancel_at_period_end: account.cancel_at_period_end,
    devices_count: account.devices.length,
    management_url: account.management_url,
  };
}
