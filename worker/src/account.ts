import type { Env } from "./env";
import { DAY, getJson, putJson } from "./kv";
import { deleteLicense, writeLicense } from "./license";
import { maskEmail } from "./validate";

export type SubStatus =
  | "created"
  | "authenticated"
  | "active"
  | "pending"
  | "halted"
  | "cancelled"
  | "completed"
  | "paused"
  | "expired";

export interface Device {
  device_id: string;
  bound_at: number;
}

export interface Account {
  email: string;
  subscription_id: string;
  status: SubStatus;
  current_end: number | null;
  cancel_at_period_end: boolean;
  devices: Device[];
  last_event_at: number | null;
}

export type SubState = Pick<Account, "status" | "current_end" | "cancel_at_period_end">;

export const MAX_DEVICES = 3;
export const PENDING_GRACE_SECONDS = 3 * DAY;
export const LIVE_STATUSES: ReadonlySet<SubStatus> = new Set(["authenticated", "active", "pending"]);

const accountKey = (email: string) => `account:${email}`;
const deviceKey = (deviceId: string) => `device:${deviceId}`;

export function entitlementExpiry(s: SubState): number | null {
  if (s.current_end == null) return null;
  switch (s.status) {
    case "active":
      return s.current_end;
    case "pending":
      return s.current_end + PENDING_GRACE_SECONDS;
    case "cancelled":
      return s.cancel_at_period_end ? s.current_end : null;
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

export function putAccount(env: Env, account: Account): Promise<void> {
  return putJson(env.LICENSES, accountKey(account.email), account);
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

export async function applyLicenses(
  env: Env,
  subscriptionId: string,
  state: SubState,
  deviceIds: string[]
): Promise<number | null> {
  const expires = entitlementExpiry(state);
  for (const id of deviceIds) {
    if (expires == null) await deleteLicense(env, id);
    else await writeLicense(env, id, expires, subscriptionId);
  }
  return expires;
}

export function syncLicenses(env: Env, account: Account): Promise<number | null> {
  return applyLicenses(
    env,
    account.subscription_id,
    account,
    account.devices.map((d) => d.device_id)
  );
}

export function accountView(account: Account) {
  return {
    email: maskEmail(account.email),
    status: account.status,
    current_end: account.current_end,
    cancel_at_period_end: account.cancel_at_period_end,
    devices_count: account.devices.length,
  };
}
