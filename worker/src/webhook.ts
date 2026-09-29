import {
  type Account,
  type SubStatus,
  LIVE_STATUSES,
  applyLicenses,
  bindDevice,
  entitlementExpiry,
  getAccount,
  isEntitled,
  putAccount,
  syncLicenses,
} from "./account";
import { verifyRazorpaySignature } from "./crypto";
import type { Env } from "./env";
import { legacyError, legacyJson } from "./http";
import { getJson, nowSecs } from "./kv";
import { LICENSE_TTL_SECONDS } from "./license";
import { normalizeEmail } from "./validate";

const EVENT_STATUS: Record<string, SubStatus> = {
  "subscription.authenticated": "authenticated",
  "subscription.activated": "active",
  "subscription.charged": "active",
  "subscription.resumed": "active",
  "subscription.pending": "pending",
  "subscription.halted": "halted",
  "subscription.cancelled": "cancelled",
  "subscription.completed": "completed",
  "subscription.paused": "paused",
};

interface SubscriptionEntity {
  id: string;
  status?: string;
  notes?: { device_id?: string; email?: string } | unknown[];
  current_end?: number | null;
}

interface WebhookEvent {
  event?: string;
  created_at?: number;
  payload?: {
    subscription?: { entity?: SubscriptionEntity };
    payment?: { entity?: { email?: string } };
  };
}

export interface PendingCheckout {
  device_id: string;
  email: string;
}

export const pendingKey = (subscriptionId: string) => `pending:${subscriptionId}`;

function outcome(status: SubStatus, expires: number | null): Response {
  if (status === "authenticated") return legacyJson({ ok: true, action: "recorded" });
  if (expires == null) return legacyJson({ ok: true, action: "deactivated" });
  if (status === "pending") return legacyJson({ ok: true, action: "grace", expires });
  if (status === "cancelled") return legacyJson({ ok: true, action: "active_until_period_end", expires });
  return legacyJson({ ok: true, action: "activated" });
}

export async function handleRazorpayWebhook(request: Request, env: Env): Promise<Response> {
  const body = await request.text();
  const signature = request.headers.get("X-Razorpay-Signature") || "";
  if (!(await verifyRazorpaySignature(body, signature, env.RAZORPAY_WEBHOOK_SECRET))) {
    return legacyError("Invalid signature", 401);
  }

  let event: WebhookEvent;
  try {
    event = JSON.parse(body);
  } catch {
    return legacyError("Invalid JSON body", 400);
  }

  const sub = event?.payload?.subscription?.entity;
  if (!sub || typeof sub.id !== "string") {
    return legacyJson({ ok: true, message: "No device_id in notes, skipped" });
  }

  // Razorpay sends empty notes as [] rather than {}.
  const notes = sub.notes && !Array.isArray(sub.notes) ? sub.notes : {};
  const pending = await getJson<PendingCheckout>(env.LICENSES, pendingKey(sub.id));
  const deviceId = typeof notes.device_id === "string" && notes.device_id ? notes.device_id : pending?.device_id;
  const email = normalizeEmail(notes.email ?? pending?.email ?? event.payload?.payment?.entity?.email);

  if (!deviceId && !email) {
    return legacyJson({ ok: true, message: "No device_id in notes, skipped" });
  }

  const status = EVENT_STATUS[event.event ?? ""];
  if (!status) return legacyJson({ ok: true, message: "Event ignored" });

  const now = nowSecs();
  const createdAt = typeof event.created_at === "number" ? event.created_at : null;
  const currentEnd =
    typeof sub.current_end === "number" ? sub.current_end : null;

  if (!email) {
    // Pre-account subscriptions (no email in notes): license only, no ordering data.
    if (status === "authenticated") return legacyJson({ ok: true, message: "Event ignored" });
    const state = {
      status,
      current_end: currentEnd ?? (status === "active" ? now + LICENSE_TTL_SECONDS : null),
      cancel_at_period_end: false,
    };
    return outcome(status, await applyLicenses(env, sub.id, state, deviceId ? [deviceId] : []));
  }

  let account = await getAccount(env, email);
  let firstSighting = false;

  if (account && account.subscription_id !== sub.id) {
    if (isEntitled(account, now) && !LIVE_STATUSES.has(status)) {
      return legacyJson({ ok: true, message: "Superseded subscription ignored" });
    }
    account.subscription_id = sub.id;
    account.cancel_at_period_end = false;
    account.last_event_at = null;
    account.current_end = null;
    firstSighting = true;
  }

  if (account && createdAt != null && account.last_event_at != null && createdAt < account.last_event_at) {
    return legacyJson({ ok: true, message: "Stale event ignored" });
  }

  if (!account) {
    account = {
      email,
      subscription_id: sub.id,
      status,
      current_end: null,
      cancel_at_period_end: false,
      devices: [],
      last_event_at: null,
    } satisfies Account;
    firstSighting = true;
  }

  account.status = status;
  account.current_end =
    currentEnd ?? account.current_end ?? (status === "active" ? now + LICENSE_TTL_SECONDS : null);
  if (createdAt != null) account.last_event_at = Math.max(createdAt, account.last_event_at ?? 0);
  if (firstSighting && deviceId) await bindDevice(env, account, deviceId, now);

  await putAccount(env, account);
  if (status === "authenticated") return outcome(status, entitlementExpiry(account));
  return outcome(status, await syncLicenses(env, account));
}
