import type { Env } from "./env";
import { getJson, putJson } from "./kv";
import { deleteLicense, writeLicense } from "./license";
import { maskEmail } from "./validate";

export type SubStatus = "active" | "cancelled" | "billing_issue" | "expired" | "refunded";

export interface Device {
  device_id: string;
  bound_at: number;
}

export interface Account {
  email: string;
  app_user_id: string;
  status: SubStatus;
  /** Epoch seconds the current paid period ends. */
  current_end: number | null;
  /** Epoch seconds a billing-issue grace period ends, when one is configured. */
  grace_end: number | null;
  cancel_at_period_end: boolean;
  management_url: string | null;
  devices: Device[];
  /** RevenueCat `event_timestamp_ms` of the newest applied event. */
  last_event_at: number | null;
}

export type SubState = Pick<Account, "status" | "current_end" | "grace_end">;

export const MAX_DEVICES = 3;

const accountKey = (email: string) => `account:${email}`;
const deviceKey = (deviceId: string) => `device:${deviceId}`;
export const customerKey = (appUserId: string) => `rcuser:${appUserId}`;

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
  await env.LICENSES.put(customerKey(account.app_user_id), account.email);
}

export async function accountForCustomer(env: Env, appUserId: string): Promise<Account | null> {
  const email = await env.LICENSES.get(customerKey(appUserId));
  if (!email) return null;
  const account = await getAccount(env, email);
  return account?.app_user_id === appUserId ? account : null;
}

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
    else await writeLicense(env, device_id, expires, account.app_user_id);
  }
  return expires;
}

export function accountView(account: Account) {
  return {
    email: maskEmail(account.email),
    status: account.status,
    current_end: account.current_end,
    cancel_at_period_end: account.cancel_at_period_end,
    devices_count: account.devices.length,
    management_url: account.management_url,
  };
}
