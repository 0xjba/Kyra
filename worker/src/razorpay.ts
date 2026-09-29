import type { Env } from "./env";

const API = "https://api.razorpay.com/v1";

export interface RazorpaySubscription {
  id: string;
  status?: string;
  short_url?: string;
  current_end?: number | null;
}

export class ProviderError extends Error {}

async function call(
  env: Env,
  path: string,
  body: Record<string, unknown>
): Promise<RazorpaySubscription> {
  let res: Response;
  try {
    res = await fetch(`${API}${path}`, {
      method: "POST",
      headers: {
        Authorization: `Basic ${btoa(`${env.RAZORPAY_KEY_ID}:${env.RAZORPAY_KEY_SECRET}`)}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
  } catch {
    throw new ProviderError("Razorpay unreachable");
  }
  if (!res.ok) throw new ProviderError(`Razorpay responded ${res.status}`);

  let data: unknown;
  try {
    data = await res.json();
  } catch {
    throw new ProviderError("Razorpay returned invalid JSON");
  }
  const sub = data as RazorpaySubscription;
  if (!sub || typeof sub.id !== "string") throw new ProviderError("Razorpay returned no subscription");
  return sub;
}

export async function createSubscription(
  env: Env,
  deviceId: string,
  email: string
): Promise<RazorpaySubscription> {
  const sub = await call(env, "/subscriptions", {
    plan_id: env.RAZORPAY_PLAN_ID,
    total_count: 120,
    quantity: 1,
    customer_notify: 1,
    notes: { device_id: deviceId, email },
  });
  if (typeof sub.short_url !== "string") throw new ProviderError("Razorpay returned no short_url");
  return sub;
}

export function cancelSubscriptionAtCycleEnd(env: Env, id: string): Promise<RazorpaySubscription> {
  return call(env, `/subscriptions/${encodeURIComponent(id)}/cancel`, { cancel_at_cycle_end: 1 });
}
