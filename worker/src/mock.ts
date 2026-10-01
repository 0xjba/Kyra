import { accountForCustomer } from "./account";
import { randomHex } from "./crypto";
import type { Env } from "./env";
import { legacyError } from "./http";
import { DAY, getJson, nowSecs } from "./kv";
import { type PendingCheckout, type RcEvent, handleRevenueCatWebhook, pendingKey } from "./webhook";

const LOCAL_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);
const MOCK_EVENTS = new Set(["INITIAL_PURCHASE", "RENEWAL", "CANCELLATION", "UNCANCELLATION", "BILLING_ISSUE", "EXPIRATION"]);

// Mock mode needs both the .dev.vars flag and a loopback request host, so a
// stray flag on a deployed worker still cannot fake payments.
export function mockEnabled(env: Env, request: Request): boolean {
  return env.DEV_MOCK_REVENUECAT === "1" && LOCAL_HOSTS.has(new URL(request.url).hostname);
}

export function mockPurchaseLink(request: Request, appUserId: string): string {
  return `${new URL(request.url).origin}/dev/mock-pay?app_user_id=${encodeURIComponent(appUserId)}`;
}

export function mockManageLink(request: Request, appUserId: string): string {
  return `${new URL(request.url).origin}/dev/mock-manage?app_user_id=${encodeURIComponent(appUserId)}`;
}

const page = (title: string, body: string) =>
  new Response(
    `<!doctype html><meta charset="utf-8"><title>${title}</title><body style="font:15px system-ui;padding:40px"><h2>${title}</h2>${body}</body>`,
    { headers: { "Content-Type": "text/html; charset=utf-8" } }
  );

// Fires a correctly authorised RevenueCat webhook through the real handler.
export async function handleMockPay(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const appUserId = url.searchParams.get("app_user_id") ?? "";
  const type = url.searchParams.get("type") ?? "INITIAL_PURCHASE";
  const days = Number(url.searchParams.get("days") ?? 30);
  if (!MOCK_EVENTS.has(type) || !Number.isFinite(days)) return legacyError("Unknown mock event", 400);

  const pending = await getJson<PendingCheckout>(env.LICENSES, pendingKey(appUserId));
  const account = await accountForCustomer(env, appUserId);
  if (!pending && !account) return legacyError("Unknown mock customer", 404);

  const nowMs = nowSecs() * 1000;
  const renews = type === "INITIAL_PURCHASE" || type === "RENEWAL";
  const event: RcEvent = {
    type,
    id: `mock_${randomHex(8)}`,
    app_user_id: appUserId,
    environment: "PRODUCTION",
    event_timestamp_ms: Date.now(),
    expiration_at_ms: renews ? nowMs + days * DAY * 1000 : (account?.current_end ?? nowSecs()) * 1000,
    ...(type === "CANCELLATION" && { cancel_reason: "UNSUBSCRIBE" }),
    ...(type === "BILLING_ISSUE" && { grace_period_expiration_at_ms: nowMs + 3 * DAY * 1000 }),
    subscriber_attributes: pending ? { $email: { value: pending.email } } : {},
  };
  const res = await handleRevenueCatWebhook(
    new Request(`${url.origin}/webhook/revenuecat`, {
      method: "POST",
      headers: { Authorization: env.REVENUECAT_WEBHOOK_AUTH, "Content-Type": "application/json" },
      body: JSON.stringify({ api_version: "1.0", event }),
    }),
    env
  );
  const result = `${type}: ${res.status} ${await res.text()}`;
  console.log(`[dev] mock webhook for ${appUserId}: ${result}`);
  return page("Mock RevenueCat", `<p>${result.replace(/</g, "&lt;")}</p><p>Return to Kyra.</p>`);
}

export function handleMockManage(request: Request): Response {
  const appUserId = new URL(request.url).searchParams.get("app_user_id") ?? "";
  const link = (type: string, label: string) =>
    `<li><a href="/dev/mock-pay?app_user_id=${encodeURIComponent(appUserId)}&type=${type}">${label}</a></li>`;
  return page(
    "Mock subscription portal",
    `<ul>${link("CANCELLATION", "Cancel renewal")}${link("UNCANCELLATION", "Resume renewal")}${link("RENEWAL", "Renew now")}${link("BILLING_ISSUE", "Fail the next payment")}${link("EXPIRATION", "Expire now")}</ul>`
  );
}
