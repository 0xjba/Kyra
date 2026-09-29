import { createHash, createHmac } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker from "../src/index";

class MemoryKV {
  store = new Map<string, { value: string; ttl?: number }>();

  async get(key: string): Promise<string | null> {
    return this.store.get(key)?.value ?? null;
  }

  async put(key: string, value: string, opts?: { expirationTtl?: number }): Promise<void> {
    const ttl = opts?.expirationTtl;
    if (ttl !== undefined && ttl < 60) throw new Error(`Invalid expiration_ttl of ${ttl}`);
    this.store.set(key, { value, ttl });
  }

  async delete(key: string): Promise<void> {
    this.store.delete(key);
  }

  json(key: string): any {
    const raw = this.store.get(key)?.value;
    return raw === undefined ? undefined : JSON.parse(raw);
  }
}

const SECRET = "whsec_test";
const NOW = 1_800_000_000;
const DAY = 86_400;
const EMAIL = "jane@gmail.com";
const dev = (n: number) => createHash("sha256").update(`device-${n}`).digest("hex");
const D1 = dev(1);

let kv: MemoryKV;
let env: any;
let fetchMock: ReturnType<typeof vi.fn>;
let razorpay: ReturnType<typeof vi.fn>;
let resend: ReturnType<typeof vi.fn>;
let emails: { to: string[]; subject: string; text: string }[];

function setTime(t: number) {
  vi.setSystemTime(t * 1000);
}

function call(path: string, init?: RequestInit & { ip?: string; host?: string }): Promise<Response> {
  const headers = new Headers(init?.headers);
  headers.set("CF-Connecting-IP", init?.ip ?? "203.0.113.1");
  const host = init?.host ?? "https://worker.test";
  return worker.fetch(new Request(`${host}${path}`, { ...init, headers }), env);
}

function post(path: string, body: unknown, opts: { ip?: string; host?: string } = {}) {
  return call(path, { method: "POST", body: JSON.stringify(body), ...opts });
}

function sign(body: string, secret = SECRET): string {
  return createHmac("sha256", secret).update(body).digest("hex");
}

function webhook(
  event: string,
  entity: Record<string, unknown>,
  createdAt: number | undefined,
  sig?: string
) {
  const body = JSON.stringify({ event, created_at: createdAt, payload: { subscription: { entity } } });
  return call("/webhook/razorpay", {
    method: "POST",
    headers: { "X-Razorpay-Signature": sig ?? sign(body) },
    body,
  });
}

function sub(overrides: Record<string, unknown> = {}) {
  return {
    id: "sub_A",
    status: "active",
    notes: { device_id: D1, email: EMAIL },
    current_end: NOW + 30 * DAY,
    ...overrides,
  };
}

async function license(deviceId: string) {
  return (await call(`/license?device_id=${deviceId}`)).json();
}

async function activate(overrides: Record<string, unknown> = {}, createdAt = NOW) {
  return webhook("subscription.activated", sub(overrides), createdAt);
}

function lastCode(): string {
  return /(\d{6})/.exec(emails.at(-1)!.subject)![1];
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  setTime(NOW);
  kv = new MemoryKV();
  env = {
    LICENSES: kv,
    JEV_API_KEY: "jev-key",
    JEV_API_URL: "https://jev.upstream/score",
    RAZORPAY_WEBHOOK_SECRET: SECRET,
    RAZORPAY_KEY_ID: "rzp_test_key",
    RAZORPAY_KEY_SECRET: "rzp_test_secret",
    RAZORPAY_PLAN_ID: "plan_test",
    RESEND_API_KEY: "re_test",
    MAIL_FROM: "Kyra <hello@kyra.test>",
  };
  emails = [];
  razorpay = vi.fn(async (url: string, init: RequestInit) => {
    if (url.endsWith("/cancel")) {
      return Response.json({ id: "sub_A", status: "active", current_end: NOW + 30 * DAY });
    }
    return Response.json({ id: "sub_new", status: "created", short_url: "https://rzp.io/i/abc" });
  });
  resend = vi.fn(async (_url: string, init: RequestInit) => {
    emails.push(JSON.parse(init.body as string));
    return Response.json({ id: "email_1" });
  });
  fetchMock = vi.fn(async (url: string, init: RequestInit) => {
    if (url.startsWith("https://api.razorpay.com/")) return razorpay(url, init);
    if (url.startsWith("https://api.resend.com/")) return resend(url, init);
    throw new Error(`unexpected fetch ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("POST /checkout/create", () => {
  it("creates a Razorpay subscription and records the pending checkout", async () => {
    const res = await post("/checkout/create", { device_id: D1, email: "  Jane@Gmail.com " });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ short_url: "https://rzp.io/i/abc", subscription_id: "sub_new" });

    const [url, init] = razorpay.mock.calls[0];
    expect(url).toBe("https://api.razorpay.com/v1/subscriptions");
    expect(init.method).toBe("POST");
    expect(init.headers.Authorization).toBe(`Basic ${btoa("rzp_test_key:rzp_test_secret")}`);
    expect(JSON.parse(init.body)).toEqual({
      plan_id: "plan_test",
      total_count: 120,
      quantity: 1,
      customer_notify: 1,
      notes: { device_id: D1, email: EMAIL },
    });

    expect(kv.json("pending:sub_new")).toEqual({ device_id: D1, email: EMAIL });
    expect(kv.store.get("pending:sub_new")!.ttl).toBe(7 * DAY);
  });

  it.each([
    [{ device_id: D1 }, "invalid_email"],
    [{ device_id: D1, email: "not-an-email" }, "invalid_email"],
    [{ email: EMAIL }, "invalid_device_id"],
    [{ email: EMAIL, device_id: "short" }, "invalid_device_id"],
    [{ email: EMAIL, device_id: D1.toUpperCase() }, "invalid_device_id"],
  ])("rejects %j with 400 %s", async (body, code) => {
    const res = await post("/checkout/create", body);
    expect(res.status).toBe(400);
    expect((await res.json()).code).toBe(code);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects bad JSON with 400", async () => {
    const res = await call("/checkout/create", { method: "POST", body: "{nope" });
    expect(res.status).toBe(400);
    expect(await res.json()).toEqual({ error: "Invalid JSON body", code: "invalid_json" });
  });

  it("maps Razorpay errors and outages to 502 without leaking details", async () => {
    razorpay.mockResolvedValueOnce(
      Response.json({ error: { description: "The api key provided is invalid" } }, { status: 401 })
    );
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect(res.status).toBe(502);
    const body = await res.json();
    expect(body.code).toBe("payment_provider_error");
    expect(JSON.stringify(body)).not.toMatch(/api key|rzp_test/);

    razorpay.mockRejectedValueOnce(new TypeError("fetch failed"));
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(502);
    razorpay.mockResolvedValueOnce(Response.json({ id: "sub_x" }));
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(502);
    expect([...kv.store.keys()].some((k) => k.startsWith("pending:"))).toBe(false);
  });

  it("rate limits checkout creation per IP", async () => {
    for (let i = 0; i < 10; i++) {
      expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(200);
    }
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect(res.status).toBe(429);
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL }, { ip: "198.51.100.9" })).status).toBe(200);
  });
});

describe("webhook lifecycle", () => {
  it("activated writes the license, the account and the device index", async () => {
    const res = await activate();
    expect(await res.json()).toEqual({ ok: true, action: "activated" });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toEqual({
      email: EMAIL,
      subscription_id: "sub_A",
      status: "active",
      current_end: NOW + 30 * DAY,
      cancel_at_period_end: false,
      devices: [{ device_id: D1, bound_at: NOW }],
      last_event_at: NOW,
    });
    expect(await kv.get(`device:${D1}`)).toBe(EMAIL);
  });

  it("uses the pending checkout when notes are missing and lower-cases emails", async () => {
    await post("/checkout/create", { device_id: D1, email: "Jane@Gmail.com" });
    await webhook("subscription.activated", sub({ id: "sub_new", notes: [] }), NOW);
    expect(kv.json(`account:${EMAIL}`).subscription_id).toBe("sub_new");
    expect((await license(D1)).active).toBe(true);
  });

  it("authenticated records the account without granting a license", async () => {
    const res = await webhook("subscription.authenticated", sub({ current_end: null }), NOW);
    expect(await res.json()).toEqual({ ok: true, action: "recorded" });
    expect(kv.json(`account:${EMAIL}`).status).toBe("authenticated");
    expect(await license(D1)).toEqual({ active: false, expires: null });
    await activate({}, NOW + 5);
    expect((await license(D1)).active).toBe(true);
  });

  it("charged extends every bound device", async () => {
    await activate();
    const d2 = dev(2);
    await bindViaRestore(d2);
    await webhook("subscription.charged", sub({ current_end: NOW + 60 * DAY }), NOW + 30 * DAY);
    for (const d of [D1, d2]) {
      expect(kv.json(`license:${d}`).expires).toBe(NOW + 60 * DAY);
    }
    expect(kv.json(`account:${EMAIL}`).current_end).toBe(NOW + 60 * DAY);
  });

  it("cancelled at cycle end keeps the license until current_end", async () => {
    await activate();
    const cancel = await post("/subscription/cancel", { device_id: D1 });
    expect(cancel.status).toBe(200);

    const res = await webhook("subscription.cancelled", sub({ status: "cancelled" }), NOW + 10);
    expect(await res.json()).toEqual({
      ok: true,
      action: "active_until_period_end",
      expires: NOW + 30 * DAY,
    });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });

    setTime(NOW + 30 * DAY + 1);
    expect(await license(D1)).toEqual({ active: false, expires: NOW + 30 * DAY });
  });

  it("an immediate cancellation removes the license", async () => {
    await activate();
    await webhook("subscription.cancelled", sub({ status: "cancelled" }), NOW + 10);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`).status).toBe("cancelled");
  });

  it("pending keeps a short grace, halted ends it", async () => {
    await activate();
    const end = NOW + 30 * DAY;
    setTime(end + 60);
    const res = await webhook("subscription.pending", sub({ status: "pending" }), end + 60);
    expect(await res.json()).toEqual({ ok: true, action: "grace", expires: end + 3 * DAY });
    setTime(end + 2 * DAY);
    expect((await license(D1)).active).toBe(true);
    setTime(end + 3 * DAY + 1);
    expect((await license(D1)).active).toBe(false);

    await webhook("subscription.halted", sub({ status: "halted" }), end + 4 * DAY);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`).status).toBe("halted");
  });

  it("recovers from halted when Razorpay re-activates", async () => {
    await activate();
    await webhook("subscription.halted", sub({ status: "halted" }), NOW + 100);
    await webhook("subscription.activated", sub({ current_end: NOW + 60 * DAY }), NOW + 200);
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 60 * DAY });
  });

  it("paused removes the license and resumed restores it", async () => {
    await activate();
    await webhook("subscription.paused", sub({ status: "paused" }), NOW + 100);
    expect((await license(D1)).active).toBe(false);
    await webhook("subscription.resumed", sub(), NOW + 200);
    expect((await license(D1)).active).toBe(true);
  });

  it("completed removes the license", async () => {
    await activate();
    await webhook("subscription.completed", sub({ status: "completed" }), NOW + 100);
    expect((await license(D1)).active).toBe(false);
  });

  it("ignores stale and out-of-order events", async () => {
    await activate({}, NOW + 100);
    await webhook("subscription.cancelled", sub({ status: "cancelled" }), NOW + 300);
    const stale = await webhook("subscription.charged", sub({ current_end: NOW + 60 * DAY }), NOW + 200);
    expect(await stale.json()).toEqual({ ok: true, message: "Stale event ignored" });
    expect(kv.json(`account:${EMAIL}`).status).toBe("cancelled");
    expect((await license(D1)).active).toBe(false);

    const late = await activate({}, NOW + 50);
    expect(await late.json()).toEqual({ ok: true, message: "Stale event ignored" });
    expect((await license(D1)).active).toBe(false);
  });

  it("is idempotent for repeated deliveries", async () => {
    await activate();
    const before = kv.json(`account:${EMAIL}`);
    await activate();
    expect(kv.json(`account:${EMAIL}`)).toEqual(before);
  });

  it("does not re-bind an evicted purchase device on renewal", async () => {
    await activate();
    for (const n of [2, 3, 4]) await bindViaRestore(dev(n));
    expect(kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id)).not.toContain(D1);
    await webhook("subscription.charged", sub({ current_end: NOW + 60 * DAY }), NOW + 30 * DAY);
    expect(kv.json(`account:${EMAIL}`).devices).toHaveLength(3);
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it("ignores late events from an older subscription once a newer one is active", async () => {
    await activate();
    await webhook("subscription.activated", sub({ id: "sub_B", current_end: NOW + 40 * DAY }), NOW + 100);
    const res = await webhook("subscription.cancelled", sub({ status: "cancelled" }), NOW + 200);
    expect(await res.json()).toEqual({ ok: true, message: "Superseded subscription ignored" });
    expect(kv.json(`account:${EMAIL}`).subscription_id).toBe("sub_B");
    expect((await license(D1)).active).toBe(true);
  });

  it("writes nothing for a bad signature", async () => {
    const res = await webhook("subscription.activated", sub(), NOW, sign("other"));
    expect(res.status).toBe(401);
    expect(kv.store.size).toBe(0);
  });
});

async function bindViaRestore(deviceId: string) {
  expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
  const res = await post("/restore/verify", { email: EMAIL, code: lastCode(), device_id: deviceId });
  expect(res.status).toBe(200);
  // Keep bound_at ordering deterministic and stay under the per-email send limit.
  setTime(Math.floor(Date.now() / 1000) + 3601);
}

describe("POST /restore/start", () => {
  it("always answers 200 and emails a code only for a subscribed account", async () => {
    const unknown = await post("/restore/start", { email: "nobody@example.com" });
    expect(unknown.status).toBe(200);
    expect(await unknown.json()).toEqual({ ok: true });
    expect(resend).not.toHaveBeenCalled();
    expect(kv.store.has("restore:nobody@example.com")).toBe(false);

    await activate();
    const known = await post("/restore/start", { email: "JANE@gmail.com" });
    expect(await known.json()).toEqual({ ok: true });
    expect(resend).toHaveBeenCalledTimes(1);

    const [url, init] = resend.mock.calls[0];
    expect(url).toBe("https://api.resend.com/emails");
    expect(init.headers.Authorization).toBe("Bearer re_test");
    const mail = emails[0] as any;
    expect(mail.from).toBe("Kyra <hello@kyra.test>");
    expect(mail.to).toEqual([EMAIL]);
    expect(mail.subject).toMatch(/^Your Kyra code: \d{6}$/);
    expect(mail.text).toContain("Enter this code in Kyra to restore Pawtrol on this Mac. It expires in 10 minutes.");
    expect(mail.html).toContain(lastCode());

    const stored = kv.json(`restore:${EMAIL}`);
    expect(stored.hash).toMatch(/^[0-9a-f]{64}$/);
    expect(JSON.stringify(stored)).not.toContain(lastCode());
    expect(stored).toMatchObject({ expires: NOW + 600, attempts: 0 });
  });

  it("does not send to lapsed accounts", async () => {
    await activate();
    await webhook("subscription.halted", sub({ status: "halted" }), NOW + 1);
    expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
    expect(resend).not.toHaveBeenCalled();
  });

  it("still answers 200 when the email provider fails", async () => {
    await activate();
    resend.mockResolvedValueOnce(new Response("nope", { status: 500 }));
    expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
  });

  it("limits sends to 3 per hour per email, known or not", async () => {
    await activate();
    for (const email of [EMAIL, "nobody@example.com"]) {
      for (let i = 0; i < 3; i++) expect((await post("/restore/start", { email })).status).toBe(200);
      const res = await post("/restore/start", { email });
      expect(res.status).toBe(429);
      expect((await res.json()).code).toBe("rate_limited");
    }
    expect(resend).toHaveBeenCalledTimes(3);
    setTime(NOW + 3601);
    expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
    expect(resend).toHaveBeenCalledTimes(4);
  });

  it("limits requests to 20 per hour per client IP", async () => {
    for (let i = 0; i < 20; i++) {
      expect((await post("/restore/start", { email: `u${i}@example.com` })).status).toBe(200);
    }
    expect((await post("/restore/start", { email: "u99@example.com" })).status).toBe(429);
    expect((await post("/restore/start", { email: "u99@example.com" }, { ip: "198.51.100.9" })).status).toBe(200);
  });

  it("rejects malformed input with 400", async () => {
    expect((await post("/restore/start", { email: "nope" })).status).toBe(400);
    expect((await call("/restore/start", { method: "POST", body: "[]" })).status).toBe(400);
  });
});

describe("POST /restore/verify", () => {
  beforeEach(async () => {
    await activate();
    await post("/restore/start", { email: EMAIL });
  });

  const D2 = dev(2);
  const verify = (code: string, deviceId = D2, email = EMAIL) =>
    post("/restore/verify", { email, code, device_id: deviceId });
  const wrong = () => (lastCode() === "000000" ? "111111" : "000000");

  it("binds the device and writes its license", async () => {
    const res = await verify(lastCode());
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(await license(D2)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(await kv.get(`device:${D2}`)).toBe(EMAIL);
    expect(kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id)).toEqual([D1, D2]);
    expect(kv.store.has(`restore:${EMAIL}`)).toBe(false);
  });

  it("is single use", async () => {
    const code = lastCode();
    expect((await verify(code)).status).toBe(200);
    const again = await verify(code, dev(3));
    expect(again.status).toBe(400);
    expect((await again.json()).code).toBe("invalid_code");
  });

  it("does not duplicate an already bound device", async () => {
    expect((await verify(lastCode(), D1)).status).toBe(200);
    expect(kv.json(`account:${EMAIL}`).devices).toHaveLength(1);
  });

  it("locks out after 5 wrong attempts", async () => {
    const code = lastCode();
    for (let i = 0; i < 4; i++) {
      const res = await verify(wrong());
      expect(res.status).toBe(400);
      expect((await res.json()).code).toBe("invalid_code");
    }
    const fifth = await verify(wrong());
    expect(fifth.status).toBe(429);
    expect((await fifth.json()).code).toBe("too_many_attempts");
    expect(kv.store.has(`restore:${EMAIL}`)).toBe(false);
    expect((await verify(code)).status).toBe(400);
    expect(await license(D2)).toEqual({ active: false, expires: null });
  });

  it("rejects an expired code", async () => {
    setTime(NOW + 601);
    const res = await verify(lastCode());
    expect(res.status).toBe(400);
    expect((await res.json()).code).toBe("code_expired");
    expect(await license(D2)).toEqual({ active: false, expires: null });
  });

  it("rejects a code for another email", async () => {
    const res = await verify(lastCode(), D2, "other@example.com");
    expect(res.status).toBe(400);
  });

  it("refuses when the subscription lapsed after the code was sent", async () => {
    await webhook("subscription.halted", sub({ status: "halted" }), NOW + 1);
    const res = await verify(lastCode());
    expect(res.status).toBe(403);
    expect((await res.json()).code).toBe("subscription_inactive");
  });

  it("caps an account at 3 devices by evicting the oldest", async () => {
    expect((await verify(lastCode())).status).toBe(200);
    setTime(NOW + 3601);
    await bindViaRestore(dev(3));
    await bindViaRestore(dev(4));

    const devices = kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id);
    expect(devices).toEqual([D2, dev(3), dev(4)]);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(await kv.get(`device:${D1}`)).toBeNull();
    expect((await call(`/account?device_id=${D1}`)).status).toBe(404);
    expect((await license(dev(4))).active).toBe(true);
  });

  it("validates input", async () => {
    expect((await verify("12345")).status).toBe(400);
    expect((await verify(lastCode(), "nope")).status).toBe(400);
    expect(kv.json(`restore:${EMAIL}`).attempts).toBe(0);
  });
});

describe("GET /account", () => {
  it("returns the masked account for a bound device", async () => {
    await activate();
    const res = await call(`/account?device_id=${D1}`);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      email: "j***@gmail.com",
      status: "active",
      current_end: NOW + 30 * DAY,
      cancel_at_period_end: false,
      devices_count: 1,
    });
  });

  it("is 404 for unknown devices and 400 for malformed ids", async () => {
    const res = await call(`/account?device_id=${dev(9)}`);
    expect(res.status).toBe(404);
    expect(await res.json()).toEqual({ error: "No account for this device", code: "not_found" });
    expect((await call("/account?device_id=abc")).status).toBe(400);
    expect((await call("/account")).status).toBe(400);
  });
});

describe("POST /subscription/cancel", () => {
  it("cancels at cycle end and reports the account", async () => {
    await activate();
    const res = await post("/subscription/cancel", { device_id: D1 });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      email: "j***@gmail.com",
      status: "active",
      current_end: NOW + 30 * DAY,
      cancel_at_period_end: true,
      devices_count: 1,
    });
    const [url, init] = razorpay.mock.calls[0];
    expect(url).toBe("https://api.razorpay.com/v1/subscriptions/sub_A/cancel");
    expect(JSON.parse(init.body)).toEqual({ cancel_at_cycle_end: 1 });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });

    expect((await post("/subscription/cancel", { device_id: D1 })).status).toBe(200);
    expect(razorpay).toHaveBeenCalledTimes(1);
  });

  it("keeps the license when Razorpay already reports the subscription as cancelled", async () => {
    await activate();
    razorpay.mockResolvedValueOnce(Response.json({ id: "sub_A", status: "cancelled", current_end: NOW + 30 * DAY }));
    const res = await post("/subscription/cancel", { device_id: D1 });
    expect((await res.json()).status).toBe("cancelled");
    expect((await license(D1)).active).toBe(true);
  });

  it("requires a device bound to the account", async () => {
    await activate();
    const res = await post("/subscription/cancel", { device_id: dev(9) });
    expect(res.status).toBe(404);
    expect(razorpay).not.toHaveBeenCalled();
    expect((await post("/subscription/cancel", { device_id: "x" })).status).toBe(400);
  });

  it("refuses when there is no active subscription", async () => {
    await activate();
    await webhook("subscription.halted", sub({ status: "halted" }), NOW + 1);
    const res = await post("/subscription/cancel", { device_id: D1 });
    expect(res.status).toBe(409);
    expect((await res.json()).code).toBe("no_active_subscription");
    expect(razorpay).not.toHaveBeenCalled();
  });

  it("maps Razorpay failures to 502 and leaves the account unchanged", async () => {
    await activate();
    razorpay.mockResolvedValueOnce(new Response("bad", { status: 400 }));
    const res = await post("/subscription/cancel", { device_id: D1 });
    expect(res.status).toBe(502);
    expect(kv.json(`account:${EMAIL}`).cancel_at_period_end).toBe(false);
  });
});

describe("mock mode", () => {
  beforeEach(() => {
    env.DEV_MOCK_RAZORPAY = "1";
  });

  const local = { host: "http://127.0.0.1:8787" };

  it("runs checkout, payment, restore and cancel locally without external calls", async () => {
    const checkout = await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    const { short_url, subscription_id } = await checkout.json();
    expect(short_url).toBe(`http://127.0.0.1:8787/dev/mock-pay?subscription_id=${subscription_id}`);

    const pay = await call(new URL(short_url).pathname + new URL(short_url).search, local);
    expect(pay.status).toBe(200);
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });

    await post("/restore/start", { email: EMAIL }, local);
    const logged = (console.log as any).mock.calls.map((c: any[]) => c[0]).join("\n");
    const code = /restore code for jane@gmail\.com: (\d{6})/.exec(logged)![1];
    expect((await post("/restore/verify", { email: EMAIL, code, device_id: dev(2) }, local)).status).toBe(200);

    const cancel = await post("/subscription/cancel", { device_id: D1 }, local);
    expect((await cancel.json()).cancel_at_period_end).toBe(true);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("stays off for non-local hosts even with the flag set", async () => {
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect((await res.json()).short_url).toBe("https://rzp.io/i/abc");
    expect(razorpay).toHaveBeenCalledTimes(1);
    expect((await call("/dev/mock-pay?subscription_id=sub_new")).status).toBe(404);
  });

  it("is off without the flag", async () => {
    delete env.DEV_MOCK_RAZORPAY;
    await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    expect(razorpay).toHaveBeenCalledTimes(1);
    expect((await call("/dev/mock-pay?subscription_id=sub_new", local)).status).toBe(404);
  });
});
