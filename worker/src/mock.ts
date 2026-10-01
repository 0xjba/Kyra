import { type Account, accountForRef } from "./account";
import { randomHex } from "./crypto";
import type { Env } from "./env";
import { escapeHtml, legacyError } from "./http";
import { DAY, getJson, nowSecs } from "./kv";
import { PLANS, type Plan, REF_KEY, signatureHeader } from "./paddle";
import { type PaddleEvent, type PendingCheckout, handlePaddleWebhook, pendingKey } from "./webhook";

const LOCAL_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

// Each mock action is the webhook sequence Paddle sends for it.
const MOCK_ACTIONS = new Set(["purchase", "renew", "cancel", "resume", "past_due", "canceled", "refund"]);

// Mock mode needs both the .dev.vars flag and a loopback request host, so a
// stray flag on a deployed worker still cannot fake payments.
export function mockEnabled(env: Env, request: Request): boolean {
  return env.DEV_MOCK_PADDLE === "1" && LOCAL_HOSTS.has(new URL(request.url).hostname);
}

export function mockPurchaseLink(request: Request, ref: string, plan: Plan): string {
  return `${new URL(request.url).origin}/dev/mock-pay?ref=${encodeURIComponent(ref)}&plan=${plan}`;
}

export function mockManageLink(request: Request, ref: string): string {
  return `${new URL(request.url).origin}/dev/mock-manage?ref=${encodeURIComponent(ref)}`;
}

const page = (title: string, body: string) =>
  new Response(
    `<!doctype html><meta charset="utf-8"><title>${escapeHtml(title)}</title><body style="font:15px system-ui;padding:40px"><h2>${escapeHtml(title)}</h2>${body}</body>`,
    { headers: { "Content-Type": "text/html; charset=utf-8" } }
  );

const iso = (secs: number) => new Date(secs * 1000).toISOString();

function subscription(
  env: Env,
  id: string,
  customerId: string,
  ref: string,
  plan: Plan,
  status: string,
  period: { start: number; end: number } | null,
  scheduledCancelAt?: number
) {
  return {
    id,
    status,
    customer_id: customerId,
    custom_data: { [REF_KEY]: ref },
    billing_cycle: { interval: plan === "yearly" ? "year" : "month", frequency: 1 },
    current_billing_period: period && { starts_at: iso(period.start), ends_at: iso(period.end) },
    scheduled_change:
      scheduledCancelAt == null ? null : { action: "cancel", effective_at: iso(scheduledCancelAt), resume_at: null },
    items: [
      {
        status: "active",
        quantity: 1,
        price: {
          id: plan === "yearly" ? env.PADDLE_PRICE_ID_YEARLY : env.PADDLE_PRICE_ID_MONTHLY,
          billing_cycle: { interval: plan === "yearly" ? "year" : "month", frequency: 1 },
        },
      },
    ],
  };
}

// Builds the events for an action and fires each one, correctly signed, through the real handler.
export async function handleMockPay(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const ref = url.searchParams.get("ref") ?? "";
  const action = url.searchParams.get("type") ?? "purchase";
  const pending = await getJson<PendingCheckout>(env.LICENSES, pendingKey(ref));
  const account: Account | null = await accountForRef(env, ref);
  const planParam = url.searchParams.get("plan");
  const plan: Plan = PLANS.includes(planParam as Plan)
    ? (planParam as Plan)
    : (account?.plan ?? pending?.plan ?? "monthly");
  const days = Number(url.searchParams.get("days") ?? (plan === "yearly" ? 365 : 30));
  if (!MOCK_ACTIONS.has(action) || !Number.isFinite(days)) return legacyError("Unknown mock event", 400);
  if (!pending && !account) return legacyError("Unknown mock customer", 404);
  if (action !== "purchase" && !account?.subscription_id) return legacyError("No mock subscription yet", 409);

  const now = nowSecs();
  const customerId = account?.customer_id ?? pending?.customer_id ?? `ctm_mock${randomHex(11)}`;
  const purchasing = action === "purchase";
  const subId = purchasing || !account?.subscription_id ? `sub_mock${randomHex(11)}` : account.subscription_id;
  const end = account?.current_end ?? now + days * DAY;
  const fresh = { start: now, end: now + days * DAY };
  const sub = (status: string, period: { start: number; end: number } | null, cancelAt?: number) =>
    subscription(env, subId, customerId, ref, plan, status, period, cancelAt);
  const txn = (origin: string) => ({
    id: `txn_mock${randomHex(11)}`,
    status: "completed",
    origin,
    customer_id: customerId,
    subscription_id: subId,
    custom_data: { [REF_KEY]: ref },
    billing_period: { starts_at: iso(fresh.start), ends_at: iso(fresh.end) },
    items: sub("active", fresh).items,
  });

  const events: [string, unknown][] = {
    purchase: [
      ["transaction.completed", txn("api")],
      ["subscription.activated", sub("active", fresh)],
    ],
    renew: [
      ["transaction.completed", txn("subscription_recurring")],
      ["subscription.updated", sub("active", fresh)],
    ],
    cancel: [["subscription.updated", sub("active", { start: now, end }, end)]],
    resume: [["subscription.updated", sub("active", { start: now, end })]],
    past_due: [["subscription.past_due", sub("past_due", { start: now, end: now + days * DAY })]],
    canceled: [["subscription.canceled", sub("canceled", null)]],
    refund: [
      [
        "adjustment.updated",
        {
          id: `adj_mock${randomHex(11)}`,
          action: "refund",
          type: "full",
          status: "approved",
          customer_id: customerId,
          subscription_id: subId,
          transaction_id: `txn_mock${randomHex(11)}`,
        },
      ],
    ],
  }[action]! as [string, unknown][];

  const results: string[] = [];
  for (const [type, data] of events) {
    const event: PaddleEvent = {
      event_id: `evt_mock${randomHex(11)}`,
      event_type: type,
      occurred_at: new Date().toISOString(),
      notification_id: `ntf_mock${randomHex(11)}`,
      data,
    };
    const raw = JSON.stringify(event);
    const res = await handlePaddleWebhook(
      new Request(`${url.origin}/webhook/paddle`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Paddle-Signature": await signatureHeader(env.PADDLE_WEBHOOK_SECRET, raw, now),
        },
        body: raw,
      }),
      env
    );
    results.push(`${type}: ${res.status} ${await res.text()}`);
  }
  console.log(`[dev] mock ${action} for ${ref}:\n  ${results.join("\n  ")}`);
  return page(
    "Mock Paddle",
    `${results.map((r) => `<p>${escapeHtml(r)}</p>`).join("")}<p>Payment complete — you can return to Kyra.</p>`
  );
}

export function handleMockManage(request: Request): Response {
  const ref = new URL(request.url).searchParams.get("ref") ?? "";
  const link = (type: string, label: string) =>
    `<li><a href="/dev/mock-pay?ref=${encodeURIComponent(ref)}&amp;type=${type}">${escapeHtml(label)}</a></li>`;
  return page(
    "Mock customer portal",
    `<ul>${link("cancel", "Cancel at period end")}${link("resume", "Keep subscription (undo cancel)")}${link("renew", "Renew now")}${link("past_due", "Fail the next payment")}${link("canceled", "Cancel now")}${link("refund", "Refund in full")}</ul>`
  );
}
