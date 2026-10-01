import { createHash } from "node:crypto";
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

const AUTH = "Bearer whk_test_secret";
const NOW = 1_800_000_000;
const DAY = 86_400;
const EMAIL = "jane@gmail.com";
const LINK = "https://pay.rev.cat/tok123";
const RC_API = "https://api.revenuecat.com/v2/projects/proj_test";
const PORTAL = "https://sandbox-customer-portal.paddle.com/cpl_01abc?action=overview&token=pga_auth";
const PADDLE_MANAGE = "https://sandbox-customer-portal.paddle.com/cpl_01abc?action=overview&token=pga_sub";
// Imported Paddle prices keep Paddle's opaque price id as the RevenueCat product id.
const PADDLE_PRICE = "pri_01jss4bz50g1z5yw121npeb3ag";
const dev = (n: number) => createHash("sha256").update(`device-${n}`).digest("hex");
const rcId = (deviceId: string) =>
  `kyra-${createHash("sha256").update(`pawtrol-rc:${deviceId}`).digest("hex")}`;
const D1 = dev(1);
const U1 = rcId(D1);

let kv: MemoryKV;
let env: any;
let fetchMock: ReturnType<typeof vi.fn>;
let revenuecat: ReturnType<typeof vi.fn>;
let resend: ReturnType<typeof vi.fn>;
let emails: { to: string[]; subject: string; text: string }[];
let rcSubs: Record<string, any[]>;
let authenticatedUrl: () => Response;

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

function send(event: Record<string, unknown>, auth: string | null = AUTH) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (auth !== null) headers.Authorization = auth;
  return call("/webhook/revenuecat", {
    method: "POST",
    headers,
    body: JSON.stringify({ api_version: "1.0", event }),
  });
}

function rcEvent(type: string, at = NOW, overrides: Record<string, unknown> = {}) {
  return {
    type,
    id: `evt_${type}_${at}`,
    app_user_id: U1,
    original_app_user_id: U1,
    aliases: [U1],
    environment: "PRODUCTION",
    store: "PADDLE",
    product_id: PADDLE_PRICE,
    entitlement_ids: ["pawtrol"],
    event_timestamp_ms: at * 1000,
    purchased_at_ms: at * 1000,
    expiration_at_ms: (NOW + 30 * DAY) * 1000,
    subscriber_attributes: {},
    ...overrides,
  };
}

const event = (type: string, at = NOW, overrides: Record<string, unknown> = {}) =>
  send(rcEvent(type, at, overrides));

async function license(deviceId: string) {
  return (await call(`/license?device_id=${deviceId}`)).json();
}

async function purchase(at = NOW, overrides: Record<string, unknown> = {}) {
  expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(200);
  return event("INITIAL_PURCHASE", at, overrides);
}

function lastCode(): string {
  return /(\d{6})/.exec(emails.at(-1)!.subject)![1];
}

function rcSub(overrides: Record<string, unknown> = {}) {
  return {
    object: "subscription",
    id: "sub1",
    environment: "production",
    store: "paddle",
    product_id: PADDLE_PRICE,
    status: "active",
    gives_access: true,
    auto_renewal_status: "will_renew",
    current_period_starts_at: NOW * 1000,
    current_period_ends_at: (NOW + 30 * DAY) * 1000,
    ends_at: (NOW + 30 * DAY) * 1000,
    management_url: PADDLE_MANAGE,
    entitlements: { object: "list", items: [{ object: "entitlement", id: "entl1", lookup_key: "pawtrol" }] },
    ...overrides,
  };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  setTime(NOW);
  kv = new MemoryKV();
  env = {
    LICENSES: kv,
    JEV_API_KEY: "jev-key",
    JEV_API_URL: "https://jev.upstream/score",
    REVENUECAT_SECRET_API_KEY: "sk_test_key",
    REVENUECAT_PROJECT_ID: "proj_test",
    REVENUECAT_WEBHOOK_AUTH: AUTH,
    REVENUECAT_WEB_PURCHASE_LINK: LINK,
    RESEND_API_KEY: "re_test",
    MAIL_FROM: "Kyra <hello@kyra.test>",
  };
  emails = [];
  rcSubs = {};
  authenticatedUrl = () => Response.json({ object: "authenticated_management_url", management_url: PORTAL });
  revenuecat = vi.fn(async (url: string) => {
    const subs = /\/customers\/([^/]+)\/subscriptions$/.exec(url);
    if (subs) {
      const items = rcSubs[decodeURIComponent(subs[1])];
      if (!items) return Response.json({ type: "resource_missing" }, { status: 404 });
      return Response.json({ object: "list", items, next_page: null });
    }
    if (url.endsWith("/authenticated_management_url")) return authenticatedUrl();
    throw new Error(`unexpected RevenueCat call ${url}`);
  });
  resend = vi.fn(async (_url: string, init: RequestInit) => {
    emails.push(JSON.parse(init.body as string));
    return Response.json({ id: "email_1" });
  });
  fetchMock = vi.fn(async (url: string, init: RequestInit) => {
    if (url.startsWith("https://api.revenuecat.com/")) return revenuecat(url, init);
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
  it("returns the Web Purchase Link for a derived app_user_id with the email preset", async () => {
    const res = await post("/checkout/create", { device_id: D1, email: "  Jane+kyra@Gmail.com " });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toEqual({
      short_url: `${LINK}/${U1}?email=jane%2Bkyra%40gmail.com`,
      app_user_id: U1,
    });
    expect(body.short_url).not.toContain(D1);
    expect(kv.json(`pending:${U1}`)).toEqual({ device_id: D1, email: "jane+kyra@gmail.com" });
    expect(kv.store.get(`pending:${U1}`)!.ttl).toBe(7 * DAY);
    expect(fetchMock).not.toHaveBeenCalled();
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
  });

  it("rejects bad JSON with 400", async () => {
    const res = await call("/checkout/create", { method: "POST", body: "{nope" });
    expect(res.status).toBe(400);
    expect(await res.json()).toEqual({ error: "Invalid JSON body", code: "invalid_json" });
  });

  it("is a 502 without a configured https purchase link", async () => {
    for (const link of ["", "http://pay.rev.cat/tok"]) {
      env.REVENUECAT_WEB_PURCHASE_LINK = link;
      const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
      expect(res.status).toBe(502);
      expect((await res.json()).code).toBe("payment_provider_error");
    }
    expect([...kv.store.keys()].some((k) => k.startsWith("pending:"))).toBe(false);
  });

  it("refuses a Mac that is already subscribed and reuses the customer for a lapsed one", async () => {
    await purchase();
    const again = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect(again.status).toBe(409);
    expect((await again.json()).code).toBe("already_active");

    await event("EXPIRATION", NOW + 100);
    const D2 = dev(2);
    await bindViaAccount(D2);
    const res = await post("/checkout/create", { device_id: D2, email: EMAIL });
    expect((await res.json()).app_user_id).toBe(U1);
  });

  it("rate limits checkout creation per IP", async () => {
    for (let i = 0; i < 10; i++) {
      expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(200);
    }
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(429);
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL }, { ip: "198.51.100.9" })).status).toBe(200);
  });
});

// Binds a device directly, for accounts whose subscription has lapsed (restore needs an active one).
async function bindViaAccount(deviceId: string) {
  const account = kv.json(`account:${EMAIL}`);
  account.devices.push({ device_id: deviceId, bound_at: NOW });
  kv.store.set(`account:${EMAIL}`, { value: JSON.stringify(account) });
  kv.store.set(`device:${deviceId}`, { value: EMAIL });
}

async function bindViaRestore(deviceId: string) {
  expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
  const res = await post("/restore/verify", { email: EMAIL, code: lastCode(), device_id: deviceId });
  expect(res.status).toBe(200);
  // Keep bound_at ordering deterministic and stay under the per-email send limit.
  setTime(Math.floor(Date.now() / 1000) + 3601);
}

describe("webhook lifecycle", () => {
  it("INITIAL_PURCHASE writes the license, the account and both indexes", async () => {
    const res = await purchase();
    expect(await res.json()).toEqual({ ok: true, action: "activated", expires: NOW + 30 * DAY });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(kv.json(`license:${D1}`)).toEqual({
      active: true,
      expires: NOW + 30 * DAY,
      app_user_id: U1,
      environment: "PRODUCTION",
    });
    expect(kv.json(`account:${EMAIL}`)).toEqual({
      email: EMAIL,
      app_user_id: U1,
      status: "active",
      current_end: NOW + 30 * DAY,
      grace_end: null,
      cancel_at_period_end: false,
      management_url: null,
      plan: "monthly",
      environment: "PRODUCTION",
      devices: [{ device_id: D1, bound_at: NOW }],
      last_event_at: NOW * 1000,
    });
    expect(await kv.get(`device:${D1}`)).toBe(EMAIL);
    expect(await kv.get(`rcuser:${U1}`)).toBe(EMAIL);
  });

  it("falls back to the $email attribute when there is no pending checkout", async () => {
    await event("INITIAL_PURCHASE", NOW, {
      app_user_id: "kyra-other",
      subscriber_attributes: { $email: { value: " Jane@Gmail.com ", updated_at_ms: 1 } },
    });
    const account = kv.json(`account:${EMAIL}`);
    expect(account.app_user_id).toBe("kyra-other");
    expect(account.devices).toEqual([]);
  });

  it("skips customers it cannot tie to an email", async () => {
    const res = await event("INITIAL_PURCHASE");
    expect(await res.json()).toEqual({ ok: true, message: "Unknown customer, skipped" });
    expect(kv.store.size).toBe(0);
  });

  it("RENEWAL extends every bound device", async () => {
    await purchase();
    const D2 = dev(2);
    await bindViaRestore(D2);
    await event("RENEWAL", NOW + 30 * DAY, { expiration_at_ms: (NOW + 60 * DAY) * 1000 });
    for (const d of [D1, D2]) expect(kv.json(`license:${d}`).expires).toBe(NOW + 60 * DAY);
    expect(kv.json(`account:${EMAIL}`).current_end).toBe(NOW + 60 * DAY);
  });

  it("CANCELLATION keeps the license until expiry, UNCANCELLATION resumes renewal", async () => {
    await purchase();
    const res = await event("CANCELLATION", NOW + 10, { cancel_reason: "UNSUBSCRIBE" });
    expect(await res.json()).toEqual({ ok: true, action: "active_until_period_end", expires: NOW + 30 * DAY });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "cancelled", cancel_at_period_end: true });

    await event("UNCANCELLATION", NOW + 20);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "active", cancel_at_period_end: false });

    await event("CANCELLATION", NOW + 30, { cancel_reason: "UNSUBSCRIBE" });
    setTime(NOW + 30 * DAY + 1);
    expect(await license(D1)).toEqual({ active: false, expires: NOW + 30 * DAY });
  });

  it("EXPIRATION deactivates", async () => {
    await purchase();
    const res = await event("EXPIRATION", NOW + 30 * DAY, { expiration_reason: "UNSUBSCRIBE" });
    expect(await res.json()).toEqual({ ok: true, action: "deactivated" });
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`).status).toBe("expired");
  });

  it("a refund ends access immediately", async () => {
    await purchase();
    await event("CANCELLATION", NOW + 10, { cancel_reason: "CUSTOMER_SUPPORT" });
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");
  });

  it("a billing issue keeps access through the grace period", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    const grace = end + 7 * DAY;
    setTime(end);
    const issue = await event("BILLING_ISSUE", end, { grace_period_expiration_at_ms: grace * 1000 });
    expect(await issue.json()).toEqual({ ok: true, action: "grace", expires: grace });
    // The paired CANCELLATION with BILLING_ERROR is a retry, not a user cancellation.
    await event("CANCELLATION", end, { cancel_reason: "BILLING_ERROR" });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "billing_issue", cancel_at_period_end: false, grace_end: grace });

    setTime(end + 3 * DAY);
    expect((await license(D1)).active).toBe(true);
    await event("RENEWAL", end + 3 * DAY, { expiration_at_ms: (end + 30 * DAY) * 1000 });
    expect(await license(D1)).toEqual({ active: true, expires: end + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`).grace_end).toBeNull();
  });

  it("a billing issue without recovery expires", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    await event("BILLING_ISSUE", end, { grace_period_expiration_at_ms: (end + 3 * DAY) * 1000 });
    await event("EXPIRATION", end + 3 * DAY, { expiration_reason: "BILLING_ERROR" });
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it("PRODUCT_CHANGE keeps access with the new expiry", async () => {
    await purchase();
    await event("PRODUCT_CHANGE", NOW + 100, { expiration_at_ms: (NOW + 365 * DAY) * 1000 });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 365 * DAY });
    expect(kv.store.get(`license:${D1}`)!.ttl).toBeGreaterThanOrEqual(365 * DAY);
  });

  it("TRANSFER moves the account to the new customer", async () => {
    await purchase();
    const res = await event("TRANSFER", NOW + 100, {
      app_user_id: undefined,
      transferred_from: [U1],
      transferred_to: ["kyra-new"],
    });
    expect(await res.json()).toEqual({ ok: true, action: "transferred", accounts: 1 });
    expect(kv.json(`account:${EMAIL}`).app_user_id).toBe("kyra-new");
    expect(await kv.get("rcuser:kyra-new")).toBe(EMAIL);

    await event("RENEWAL", NOW + 200, { app_user_id: "kyra-new", expiration_at_ms: (NOW + 60 * DAY) * 1000 });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 60 * DAY });
    // The old customer no longer owns the account.
    const late = await event("EXPIRATION", NOW + 300);
    expect(await late.json()).toEqual({ ok: true, message: "Superseded customer ignored" });
    expect((await license(D1)).active).toBe(true);
  });

  it("ignores stale and out-of-order events", async () => {
    await purchase(NOW + 100);
    await event("EXPIRATION", NOW + 300);
    const stale = await event("RENEWAL", NOW + 200, { expiration_at_ms: (NOW + 60 * DAY) * 1000 });
    expect(await stale.json()).toEqual({ ok: true, message: "Stale event ignored" });
    expect(kv.json(`account:${EMAIL}`).status).toBe("expired");
    expect((await license(D1)).active).toBe(false);
  });

  it("is idempotent for redelivered events", async () => {
    await purchase();
    const before = kv.json(`account:${EMAIL}`);
    await event("INITIAL_PURCHASE");
    expect(kv.json(`account:${EMAIL}`)).toEqual(before);
  });

  it("does not re-bind an evicted purchase device on renewal", async () => {
    await purchase();
    for (const n of [2, 3, 4]) await bindViaRestore(dev(n));
    expect(kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id)).not.toContain(D1);
    await event("RENEWAL", NOW + 30 * DAY, { expiration_at_ms: (NOW + 60 * DAY) * 1000 });
    expect(kv.json(`account:${EMAIL}`).devices).toHaveLength(3);
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it("a purchase from another Mac with the same email takes over the account", async () => {
    await purchase();
    await event("EXPIRATION", NOW + 100);
    const D2 = dev(2);
    const U2 = rcId(D2);
    await post("/checkout/create", { device_id: D2, email: EMAIL });
    await event("INITIAL_PURCHASE", NOW + 200, { app_user_id: U2, expiration_at_ms: (NOW + 40 * DAY) * 1000 });
    const account = kv.json(`account:${EMAIL}`);
    expect(account.app_user_id).toBe(U2);
    expect(account.devices.map((d: any) => d.device_id)).toEqual([D1, D2]);
    expect((await license(D1)).active).toBe(true);
    const late = await event("CANCELLATION", NOW + 300, { cancel_reason: "UNSUBSCRIBE" });
    expect(await late.json()).toEqual({ ok: true, message: "Superseded customer ignored" });
  });

  it.each([
    ["missing", null],
    ["wrong", "Bearer nope"],
    ["prefix", AUTH.slice(0, -1)],
    ["suffix", `${AUTH}x`],
    ["bare secret", "whk_test_secret"],
  ])("rejects a %s Authorization header without writing", async (_name, auth) => {
    await post("/checkout/create", { device_id: D1, email: EMAIL });
    const before = new Map(kv.store);
    const res = await send(rcEvent("INITIAL_PURCHASE"), auth);
    expect(res.status).toBe(401);
    expect(await res.json()).toEqual({ error: "Unauthorized" });
    expect(kv.store).toEqual(before);
  });

  it("rejects everything when no webhook secret is configured", async () => {
    env.REVENUECAT_WEBHOOK_AUTH = "";
    expect((await send(rcEvent("INITIAL_PURCHASE"), "")).status).toBe(401);
  });

  it("ignores unrelated and sandbox events", async () => {
    await post("/checkout/create", { device_id: D1, email: EMAIL });
    for (const type of ["TEST", "NON_RENEWING_PURCHASE", "SUBSCRIBER_ALIAS"]) {
      expect(await (await event(type)).json()).toEqual({ ok: true, message: "Event ignored" });
    }
    const sandbox = await event("INITIAL_PURCHASE", NOW, { environment: "SANDBOX" });
    expect(await sandbox.json()).toEqual({ ok: true, message: "Sandbox event ignored" });
    expect(kv.store.has(`account:${EMAIL}`)).toBe(false);

    env.REVENUECAT_ALLOW_SANDBOX = "1";
    await event("INITIAL_PURCHASE", NOW, { environment: "SANDBOX" });
    expect((await license(D1)).active).toBe(true);
  });

  it("answers 400 for malformed JSON and skips bodies without an event", async () => {
    const bad = await call("/webhook/revenuecat", { method: "POST", headers: { Authorization: AUTH }, body: "{nope" });
    expect(bad.status).toBe(400);
    const empty = await call("/webhook/revenuecat", { method: "POST", headers: { Authorization: AUTH }, body: "{}" });
    expect(await empty.json()).toEqual({ ok: true, message: "No event, skipped" });
  });
});

describe("GET /license RevenueCat fallback", () => {
  it("asks RevenueCat when KV has no license, then caches for 10 minutes", async () => {
    rcSubs[U1] = [rcSub()];
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });

    const [url, init] = revenuecat.mock.calls[0];
    expect(url).toBe(`${RC_API}/customers/${U1}/subscriptions`);
    expect(init.headers.Authorization).toBe("Bearer sk_test_key");
    expect(kv.json(`license:${D1}`)).toEqual({
      active: true,
      expires: NOW + 30 * DAY,
      app_user_id: U1,
      environment: "PRODUCTION",
    });

    kv.store.delete(`license:${D1}`);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(revenuecat).toHaveBeenCalledTimes(1);
    setTime(NOW + 601);
    kv.store.delete(`rcsync:${D1}`);
    expect((await license(D1)).active).toBe(true);
    expect(revenuecat).toHaveBeenCalledTimes(2);
  });

  it("does not call RevenueCat while the KV license is valid", async () => {
    await purchase();
    expect((await license(D1)).active).toBe(true);
    expect(revenuecat).not.toHaveBeenCalled();
  });

  it("refreshes an expired KV license for a bound device", async () => {
    await purchase();
    rcSubs[U1] = [rcSub({ ends_at: (NOW + 90 * DAY) * 1000 })];
    setTime(NOW + 31 * DAY);
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 90 * DAY });
    expect(kv.json(`account:${EMAIL}`).management_url).toBe(PADDLE_MANAGE);
  });

  it("reports inactive when RevenueCat has no access or fails", async () => {
    expect(await license(D1)).toEqual({ active: false, expires: null });
    kv.store.delete(`rcsync:${D1}`);
    rcSubs[U1] = [rcSub({ gives_access: false, status: "expired" })];
    expect(await license(D1)).toEqual({ active: false, expires: null });
    kv.store.delete(`rcsync:${D1}`);
    revenuecat.mockResolvedValueOnce(new Response("down", { status: 503 }));
    expect(await license(D1)).toEqual({ active: false, expires: null });
    kv.store.delete(`rcsync:${D1}`);
    revenuecat.mockRejectedValueOnce(new TypeError("fetch failed"));
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it("ignores other entitlements and sandbox subscriptions", async () => {
    rcSubs[U1] = [
      rcSub({ entitlements: { items: [{ lookup_key: "other" }] } }),
      rcSub({ id: "sub2", environment: "sandbox" }),
    ];
    expect((await license(D1)).active).toBe(false);
  });

  it("grants a one-day lease during a grace period", async () => {
    rcSubs[U1] = [rcSub({ status: "in_grace_period", ends_at: (NOW - DAY) * 1000 })];
    expect(await license(D1)).toEqual({ active: true, expires: NOW + DAY });
  });

  it("never revives a device evicted from a known customer", async () => {
    await purchase();
    for (const n of [2, 3, 4]) await bindViaRestore(dev(n));
    rcSubs[U1] = [rcSub()];
    expect((await license(D1)).active).toBe(false);
    expect(revenuecat).not.toHaveBeenCalled();
  });
});

describe("POST /restore/start", () => {
  it("always answers 200 and emails a code only for a subscribed account", async () => {
    const unknown = await post("/restore/start", { email: "nobody@example.com" });
    expect(unknown.status).toBe(200);
    expect(await unknown.json()).toEqual({ ok: true });
    expect(resend).not.toHaveBeenCalled();
    expect(kv.store.has("restore:nobody@example.com")).toBe(false);

    await purchase();
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
    expect(mail.html).toContain(lastCode());

    const stored = kv.json(`restore:${EMAIL}`);
    expect(stored.hash).toMatch(/^[0-9a-f]{64}$/);
    expect(JSON.stringify(stored)).not.toContain(lastCode());
    expect(stored).toMatchObject({ expires: NOW + 600, attempts: 0 });
  });

  it("does not send to lapsed accounts", async () => {
    await purchase();
    await event("EXPIRATION", NOW + 1);
    expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
    expect(resend).not.toHaveBeenCalled();
  });

  it("still answers 200 when the email provider fails", async () => {
    await purchase();
    resend.mockResolvedValueOnce(new Response("nope", { status: 500 }));
    expect((await post("/restore/start", { email: EMAIL })).status).toBe(200);
  });

  it("limits sends to 3 per hour per email, known or not", async () => {
    await purchase();
    for (const email of [EMAIL, "nobody@example.com"]) {
      for (let i = 0; i < 3; i++) expect((await post("/restore/start", { email })).status).toBe(200);
      const res = await post("/restore/start", { email });
      expect(res.status).toBe(429);
      expect((await res.json()).code).toBe("rate_limited");
    }
    expect(resend).toHaveBeenCalledTimes(3);
  });

  it("limits requests to 20 per hour per client IP", async () => {
    for (let i = 0; i < 20; i++) {
      expect((await post("/restore/start", { email: `u${i}@example.com` })).status).toBe(200);
    }
    expect((await post("/restore/start", { email: "u99@example.com" })).status).toBe(429);
    expect((await post("/restore/start", { email: "u99@example.com" }, { ip: "198.51.100.9" })).status).toBe(200);
  });
});

describe("POST /restore/verify", () => {
  beforeEach(async () => {
    await purchase();
    await post("/restore/start", { email: EMAIL });
  });

  const D2 = dev(2);
  const verify = (code: string, deviceId = D2, email = EMAIL) =>
    post("/restore/verify", { email, code, device_id: deviceId });
  const wrong = () => (lastCode() === "000000" ? "111111" : "000000");

  it("binds the device, writes its license and returns the customer id", async () => {
    const res = await verify(lastCode());
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ active: true, expires: NOW + 30 * DAY, app_user_id: U1 });
    expect(await license(D2)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(await kv.get(`device:${D2}`)).toBe(EMAIL);
    expect(kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id)).toEqual([D1, D2]);
    expect(kv.store.has(`restore:${EMAIL}`)).toBe(false);
  });

  it("is single use", async () => {
    const code = lastCode();
    expect((await verify(code)).status).toBe(200);
    expect((await verify(code, dev(3))).status).toBe(400);
  });

  it("locks out after 5 wrong attempts", async () => {
    const code = lastCode();
    for (let i = 0; i < 4; i++) expect((await verify(wrong())).status).toBe(400);
    const fifth = await verify(wrong());
    expect(fifth.status).toBe(429);
    expect((await fifth.json()).code).toBe("too_many_attempts");
    expect((await verify(code)).status).toBe(400);
  });

  it("rejects an expired code", async () => {
    setTime(NOW + 601);
    const res = await verify(lastCode());
    expect((await res.json()).code).toBe("code_expired");
  });

  it("refuses when the subscription lapsed after the code was sent", async () => {
    await event("EXPIRATION", NOW + 1);
    const res = await verify(lastCode());
    expect(res.status).toBe(403);
    expect((await res.json()).code).toBe("subscription_inactive");
  });

  it("caps an account at 3 devices by evicting the oldest", async () => {
    expect((await verify(lastCode())).status).toBe(200);
    setTime(NOW + 3601);
    await bindViaRestore(dev(3));
    await bindViaRestore(dev(4));

    expect(kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id)).toEqual([D2, dev(3), dev(4)]);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(await kv.get(`device:${D1}`)).toBeNull();
    expect((await call(`/account?device_id=${D1}`)).status).toBe(404);
    expect((await license(dev(4))).active).toBe(true);
  });
});

describe("GET /account", () => {
  it("returns the masked account for a bound device", async () => {
    await purchase();
    const res = await call(`/account?device_id=${D1}`);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      email: "j***@gmail.com",
      status: "active",
      current_end: NOW + 30 * DAY,
      cancel_at_period_end: false,
      devices_count: 1,
      management_url: null,
      plan: "monthly",
    });
  });

  it("is 404 for unknown devices and 400 for malformed ids", async () => {
    expect((await call(`/account?device_id=${dev(9)}`)).status).toBe(404);
    expect((await call("/account?device_id=abc")).status).toBe(400);
  });
});

describe("POST /account/manage", () => {
  it("returns the authenticated portal link and remembers the subscription's management_url", async () => {
    await purchase();
    rcSubs[U1] = [rcSub()];
    const res = await post("/account/manage", { device_id: D1 });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ url: PORTAL });
    expect(revenuecat.mock.calls.map((c) => c[0])).toEqual([
      `${RC_API}/customers/${U1}/subscriptions`,
      `${RC_API}/subscriptions/sub1/authenticated_management_url`,
    ]);
    expect((await (await call(`/account?device_id=${D1}`)).json()).management_url).toBe(PADDLE_MANAGE);
  });

  it("needs a bound device and a subscription", async () => {
    await purchase();
    expect((await post("/account/manage", { device_id: dev(9) })).status).toBe(404);
    expect((await post("/account/manage", { device_id: "x" })).status).toBe(400);
    const none = await post("/account/manage", { device_id: D1 });
    expect(none.status).toBe(409);
    expect((await none.json()).code).toBe("no_active_subscription");
  });

  it("maps RevenueCat failures to 502", async () => {
    await purchase();
    revenuecat.mockResolvedValueOnce(new Response("nope", { status: 401 }));
    const res = await post("/account/manage", { device_id: D1 });
    expect(res.status).toBe(502);
    expect(JSON.stringify(await res.json())).not.toContain("sk_test");
  });
});

describe("POST /subscription/cancel", () => {
  it("is gone in favour of the management page", async () => {
    await purchase();
    const res = await post("/subscription/cancel", { device_id: D1 });
    expect(res.status).toBe(410);
    expect((await res.json()).code).toBe("use_management_url");
    expect(kv.json(`account:${EMAIL}`).cancel_at_period_end).toBe(false);
  });
});

describe("mock mode", () => {
  beforeEach(() => {
    env.DEV_MOCK_REVENUECAT = "1";
  });

  const local = { host: "http://127.0.0.1:8787" };
  const open = (url: string) => call(new URL(url).pathname + new URL(url).search, local);

  it("runs checkout, payment, restore and management locally without external calls", async () => {
    const checkout = await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    const { short_url } = await checkout.json();
    expect(short_url).toBe(`http://127.0.0.1:8787/dev/mock-pay?app_user_id=${U1}`);

    expect((await open(short_url)).status).toBe(200);
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });

    await post("/restore/start", { email: EMAIL }, local);
    const logged = (console.log as any).mock.calls.map((c: any[]) => c[0]).join("\n");
    const code = /restore code for jane@gmail\.com: (\d{6})/.exec(logged)![1];
    expect((await post("/restore/verify", { email: EMAIL, code, device_id: dev(2) }, local)).status).toBe(200);

    const { url } = await (await post("/account/manage", { device_id: D1 }, local)).json();
    expect(url).toBe(`http://127.0.0.1:8787/dev/mock-manage?app_user_id=${U1}`);
    expect(await (await open(url)).text()).toContain("type=CANCELLATION");
    setTime(NOW + 10);
    await open(`${url.replace("mock-manage", "mock-pay")}&type=CANCELLATION`);
    expect(kv.json(`account:${EMAIL}`).cancel_at_period_end).toBe(true);
    expect((await license(D1)).active).toBe(true);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("stays off for non-local hosts even with the flag set", async () => {
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect((await res.json()).short_url.startsWith(LINK)).toBe(true);
    expect((await call(`/dev/mock-pay?app_user_id=${U1}`)).status).toBe(404);
  });

  it("is off without the flag", async () => {
    delete env.DEV_MOCK_REVENUECAT;
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    expect((await res.json()).short_url.startsWith(LINK)).toBe(true);
    expect((await call(`/dev/mock-pay?app_user_id=${U1}`, local)).status).toBe(404);
  });
});

describe("Paddle as RevenueCat's payment provider", () => {
  it.each(["PADDLE", "RC_BILLING"])("handles %s-store events through the same lifecycle", async (store) => {
    await purchase(NOW, { store });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    await event("CANCELLATION", NOW + 10, { store, cancel_reason: "UNSUBSCRIBE" });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "cancelled", cancel_at_period_end: true });
    await event("EXPIRATION", NOW + 30 * DAY, { store });
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it.each([
    ["CUSTOMER_SUPPORT", undefined],
    ["REFUND", undefined],
    ["UNSUBSCRIBE", -0.99],
  ])("treats a Paddle CANCELLATION with reason %s and price %s as a refund", async (reason, price) => {
    await purchase();
    const res = await event("CANCELLATION", NOW + 10, { cancel_reason: reason, price });
    expect(await res.json()).toEqual({ ok: true, action: "deactivated" });
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "refunded", cancel_at_period_end: false });
  });

  it("keeps a paid cancellation and a billing retry apart from refunds", async () => {
    await purchase();
    await event("CANCELLATION", NOW + 10, { cancel_reason: "UNSUBSCRIBE", price: 0.99 });
    expect(kv.json(`account:${EMAIL}`).status).toBe("cancelled");
    await event("CANCELLATION", NOW + 20, { cancel_reason: "BILLING_ERROR", price: -0.99 });
    expect(kv.json(`account:${EMAIL}`).status).toBe("billing_issue");
  });

  it("a renewal after a refund that kept the subscription restores access", async () => {
    await purchase();
    await event("CANCELLATION", NOW + 10, { cancel_reason: "CUSTOMER_SUPPORT" });
    await event("RENEWAL", NOW + 30 * DAY, { expiration_at_ms: (NOW + 60 * DAY) * 1000 });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 60 * DAY });
  });
});

describe("plan", () => {
  const plan = async () => (await (await call(`/account?device_id=${D1}`)).json()).plan;

  it("derives monthly and yearly from the period of an opaque Paddle price id", async () => {
    await purchase();
    expect(await plan()).toBe("monthly");
    await event("PRODUCT_CHANGE", NOW + 100, { expiration_at_ms: (NOW + 100 + 365 * DAY) * 1000 });
    expect(await plan()).toBe("yearly");
    expect(kv.json(`account:${EMAIL}`).plan).toBe("yearly");
  });

  it("prefers a product id that names the period", async () => {
    await purchase(NOW, { product_id: "pawtrol_yearly", purchased_at_ms: undefined });
    expect(await plan()).toBe("yearly");
    // RevenueCat Billing PRODUCT_CHANGE: product_id is the old product, new_product_id the new one.
    await event("PRODUCT_CHANGE", NOW + 100, {
      product_id: "pawtrol_yearly",
      new_product_id: "pawtrol_monthly",
      purchased_at_ms: undefined,
    });
    expect(await plan()).toBe("monthly");
  });

  it("keeps the known plan when an event does not reveal one, and is null when never known", async () => {
    await purchase();
    await event("EXPIRATION", NOW + 30 * DAY);
    expect(await plan()).toBe("monthly");

    kv.store.clear();
    await purchase(NOW, { purchased_at_ms: undefined });
    expect(await plan()).toBeNull();
  });

  it("is filled in by the RevenueCat fallback", async () => {
    await purchase(NOW, { purchased_at_ms: undefined });
    rcSubs[U1] = [rcSub({ current_period_ends_at: (NOW + 365 * DAY) * 1000, ends_at: (NOW + 365 * DAY) * 1000 })];
    setTime(NOW + 31 * DAY);
    expect((await license(D1)).active).toBe(true);
    expect(kv.json(`account:${EMAIL}`).plan).toBe("yearly");
  });

  it("does not read a period out of opaque ids", async () => {
    await purchase(NOW, { product_id: "pri_01m2y4", purchased_at_ms: undefined });
    expect(await plan()).toBeNull();
  });
});

describe("environment", () => {
  async function sandboxPurchase() {
    env.REVENUECAT_ALLOW_SANDBOX = "1";
    await purchase(NOW, { environment: "SANDBOX" });
  }

  it("stores the event environment on the account and the license", async () => {
    await sandboxPurchase();
    expect(kv.json(`account:${EMAIL}`).environment).toBe("SANDBOX");
    expect(kv.json(`license:${D1}`).environment).toBe("SANDBOX");
    expect((await license(D1)).active).toBe(true);
  });

  it("treats sandbox records as absent once REVENUECAT_ALLOW_SANDBOX is removed", async () => {
    await sandboxPurchase();
    delete env.REVENUECAT_ALLOW_SANDBOX;
    rcSubs[U1] = [rcSub({ environment: "sandbox" })];

    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect((await call(`/account?device_id=${D1}`)).status).toBe(404);
    expect((await post("/account/manage", { device_id: D1 })).status).toBe(404);
    await post("/restore/start", { email: EMAIL });
    expect(resend).not.toHaveBeenCalled();
    expect((await post("/checkout/create", { device_id: dev(2), email: EMAIL })).status).toBe(200);
    const score = await post("/jev/score", { device_id: D1, questions: ["q"] });
    expect(score.status).toBe(403);
  });

  it("lets a production purchase replace a sandbox account once the flag is gone", async () => {
    await sandboxPurchase();
    delete env.REVENUECAT_ALLOW_SANDBOX;
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(200);
    await event("INITIAL_PURCHASE", NOW + 100);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ environment: "PRODUCTION", status: "active" });
    expect(kv.json(`license:${D1}`).environment).toBe("PRODUCTION");
    expect((await license(D1)).active).toBe(true);
  });

  it("counts records without an environment as sandbox", async () => {
    kv.store.set(`license:${D1}`, {
      value: JSON.stringify({ active: true, expires: NOW + DAY, app_user_id: U1 }),
    });
    kv.store.set(`rcsync:${D1}`, { value: String(NOW) });
    expect(await license(D1)).toEqual({ active: false, expires: null });
    env.REVENUECAT_ALLOW_SANDBOX = "1";
    expect(await license(D1)).toEqual({ active: true, expires: NOW + DAY });
  });

  it("ignores lifecycle events without an environment unless sandbox is allowed", async () => {
    await post("/checkout/create", { device_id: D1, email: EMAIL });
    const res = await event("INITIAL_PURCHASE", NOW, { environment: undefined });
    expect(await res.json()).toEqual({ ok: true, message: "Sandbox event ignored" });
    expect(kv.store.has(`account:${EMAIL}`)).toBe(false);
  });

  it("ignores sandbox transfers but processes transfers without an environment", async () => {
    await purchase();
    const sandbox = await event("TRANSFER", NOW + 10, {
      app_user_id: undefined,
      environment: "SANDBOX",
      transferred_from: [U1],
      transferred_to: ["kyra-new"],
    });
    expect(await sandbox.json()).toEqual({ ok: true, message: "Sandbox event ignored" });
    const plain = await event("TRANSFER", NOW + 20, {
      app_user_id: undefined,
      environment: undefined,
      transferred_from: [U1],
      transferred_to: ["kyra-new"],
    });
    expect(await plain.json()).toEqual({ ok: true, action: "transferred", accounts: 1 });
  });

  it("writes the subscription's environment when RevenueCat fills in a license", async () => {
    env.REVENUECAT_ALLOW_SANDBOX = "1";
    rcSubs[U1] = [rcSub({ environment: "sandbox" })];
    expect((await license(D1)).active).toBe(true);
    expect(kv.json(`license:${D1}`).environment).toBe("SANDBOX");
  });
});

describe("POST /checkout/create for an email that already pays", () => {
  it("is 409 already_active from another Mac", async () => {
    await purchase();
    const res = await post("/checkout/create", { device_id: dev(2), email: " JANE@gmail.com " });
    expect(res.status).toBe(409);
    expect(await res.json()).toEqual({
      code: "already_active",
      error: "This email already has an active Pawtrol subscription",
    });
    expect(kv.store.has(`pending:${rcId(dev(2))}`)).toBe(false);
  });

  it("still says this Mac when the requesting Mac is the subscribed one", async () => {
    await purchase();
    const res = await post("/checkout/create", { device_id: D1, email: "other@example.com" });
    expect(await res.json()).toEqual({ code: "already_active", error: "Pawtrol is already active on this Mac" });
  });

  it("allows checkout once the email's subscription has lapsed", async () => {
    await purchase();
    await event("EXPIRATION", NOW + 100);
    expect((await post("/checkout/create", { device_id: dev(2), email: EMAIL })).status).toBe(200);
  });
});

describe("POST /account/manage with Paddle subscriptions", () => {
  beforeEach(async () => {
    await purchase();
    rcSubs[U1] = [rcSub()];
  });

  const manage = () => post("/account/manage", { device_id: D1 });

  it("uses the authenticated Paddle customer portal URL", async () => {
    expect(await (await manage()).json()).toEqual({ url: PORTAL });
  });

  it.each([
    ["an error", () => Response.json({ type: "invalid_request" }, { status: 400 })],
    ["a 404", () => Response.json({ type: "resource_missing" }, { status: 404 })],
    ["null", () => Response.json({ object: "authenticated_management_url", management_url: null })],
    ["a non-https URL", () => Response.json({ management_url: "http://customer-portal.paddle.com/x" })],
  ])("falls back to the subscription's management_url when the endpoint returns %s", async (_name, reply) => {
    authenticatedUrl = reply;
    const res = await manage();
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ url: PADDLE_MANAGE });
  });

  it("is 502 when the endpoint fails and there is no management_url, 409 when neither has a URL", async () => {
    authenticatedUrl = () => new Response("down", { status: 503 });
    rcSubs[U1] = [rcSub({ management_url: null })];
    expect((await manage()).status).toBe(502);

    authenticatedUrl = () => Response.json({ management_url: null });
    rcSubs[U1] = [rcSub({ management_url: "javascript:alert(1)" })];
    const none = await manage();
    expect(none.status).toBe(409);
    expect((await none.json()).code).toBe("no_active_subscription");
  });
});

describe("REVENUECAT_ENTITLEMENT", () => {
  const kyraSub = (overrides: Record<string, unknown> = {}) =>
    rcSub({ entitlements: { items: [{ lookup_key: "Kyra_pawtrol" }] }, ...overrides });

  it("uses the configured entitlement for the license fallback and Manage", async () => {
    env.REVENUECAT_ENTITLEMENT = "Kyra_pawtrol";
    rcSubs[U1] = [rcSub({ id: "sub_default" }), kyraSub({ id: "sub_kyra" })];
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });

    kv.store.delete(`license:${D1}`);
    await purchase();
    expect((await post("/account/manage", { device_id: D1 })).status).toBe(200);
    expect(revenuecat.mock.calls.at(-1)![0]).toBe(`${RC_API}/subscriptions/sub_kyra/authenticated_management_url`);
  });

  it("does not count the default entitlement when another one is configured", async () => {
    env.REVENUECAT_ENTITLEMENT = "Kyra_pawtrol";
    rcSubs[U1] = [rcSub()];
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it("falls back to pawtrol when unset or blank", async () => {
    env.REVENUECAT_ENTITLEMENT = "  ";
    rcSubs[U1] = [kyraSub()];
    expect((await license(D1)).active).toBe(false);
    kv.store.delete(`rcsync:${D1}`);
    rcSubs[U1] = [rcSub()];
    expect((await license(D1)).active).toBe(true);
  });
});
