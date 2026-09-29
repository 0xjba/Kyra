import { hmacHex, randomHex } from "./crypto";
import type { Env } from "./env";
import { legacyError } from "./http";
import { DAY, getJson, nowSecs } from "./kv";
import { type PendingCheckout, handleRazorpayWebhook, pendingKey } from "./webhook";

const LOCAL_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

// Mock mode needs both the .dev.vars flag and a loopback request host, so a
// stray flag on a deployed worker still cannot fake payments.
export function mockEnabled(env: Env, request: Request): boolean {
  return env.DEV_MOCK_RAZORPAY === "1" && LOCAL_HOSTS.has(new URL(request.url).hostname);
}

export function mockSubscription(request: Request) {
  const id = `sub_mock_${randomHex(8)}`;
  const origin = new URL(request.url).origin;
  return { id, short_url: `${origin}/dev/mock-pay?subscription_id=${id}` };
}

export async function handleMockPay(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const id = url.searchParams.get("subscription_id") ?? "";
  const pending = await getJson<PendingCheckout>(env.LICENSES, pendingKey(id));
  if (!pending) return legacyError("Unknown mock subscription", 404);

  const now = nowSecs();
  const entity = {
    id,
    status: "active",
    notes: { device_id: pending.device_id, email: pending.email },
    current_start: now,
    current_end: now + 30 * DAY,
  };
  const results: string[] = [];
  for (const [i, event] of ["subscription.activated", "subscription.charged"].entries()) {
    const body = JSON.stringify({ event, created_at: now + i, payload: { subscription: { entity } } });
    const res = await handleRazorpayWebhook(
      new Request(`${url.origin}/webhook/razorpay`, {
        method: "POST",
        headers: { "X-Razorpay-Signature": await hmacHex(body, env.RAZORPAY_WEBHOOK_SECRET) },
        body,
      }),
      env
    );
    results.push(`${event}: ${res.status}`);
  }
  console.log(`[dev] mock payment for ${id}: ${results.join(", ")}`);

  return new Response(
    `<!doctype html><meta charset="utf-8"><title>Mock payment</title><body style="font:15px system-ui;padding:40px"><h2>Mock payment complete</h2><p>${results.join("<br>")}</p><p>Return to Kyra.</p></body>`,
    { headers: { "Content-Type": "text/html; charset=utf-8" } }
  );
}
