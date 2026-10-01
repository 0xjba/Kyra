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

const SECRET = "pdl_ntfset_01test_webhooksecret";
const NOW = 1_800_000_000;
const DAY = 86_400;
const EMAIL = "jane@gmail.com";
const SANDBOX_API = "https://sandbox-api.paddle.com";
const PRICE_M = "pri_monthly000000000000000000";
const PRICE_Y = "pri_yearly0000000000000000000";
const CUS1 = "ctm_01hv6y1jedq4p1n0yqn5ba3ky4";
const SUB1 = "sub_01hv8x29kz0t586xy6zn1a62ny";
const SUB2 = "sub_02hv8x29kz0t586xy6zn1a62ny";
const PORTAL_SUB = "https://sandbox-customer-portal.paddle.com/cpl_1?action=view_subscription&subscription_id=sub&token=pga_x";
const PORTAL_OVERVIEW = "https://sandbox-customer-portal.paddle.com/cpl_1?action=overview&token=pga_x";
const dev = (n: number) => createHash("sha256").update(`device-${n}`).digest("hex");
const refOf = (deviceId: string) =>
  `kyra-${createHash("sha256").update(`pawtrol-ref:${deviceId}`).digest("hex")}`;
const D1 = dev(1);
const R1 = refOf(D1);
const iso = (secs: number) => new Date(secs * 1000).toISOString();

let kv: MemoryKV;
let env: any;
let fetchMock: ReturnType<typeof vi.fn>;
let paddle: ReturnType<typeof vi.fn>;
let resend: ReturnType<typeof vi.fn>;
let emails: { to: string[]; subject: string; text: string }[];
let customers: Map<string, string>;
let transactions: Map<string, any>;
let subscriptions: Map<string, any>;
let paddleCalls: { method: string; path: string; body: any; headers: Record<string, string> }[];
let txnCounter: number;
let transactionResponse: (() => Response) | null;

function setTime(t: number) {
  vi.setSystemTime(t * 1000);
}
const nowSecs = () => Math.floor(Date.now() / 1000);

function call(path: string, init?: RequestInit & { ip?: string; host?: string }): Promise<Response> {
  const headers = new Headers(init?.headers);
  headers.set("CF-Connecting-IP", init?.ip ?? "203.0.113.1");
  const host = init?.host ?? "https://worker.test";
  return worker.fetch(new Request(`${host}${path}`, { ...init, headers }), env);
}

function post(path: string, body: unknown, opts: { ip?: string; host?: string } = {}) {
  return call(path, { method: "POST", body: JSON.stringify(body), ...opts });
}

const sign = (raw: string, ts = nowSecs(), secret = SECRET) =>
  `ts=${ts};h1=${createHmac("sha256", secret).update(`${ts}:${raw}`).digest("hex")}`;

let eventSeq = 0;
function rawEvent(type: string, data: unknown, at = nowSecs(), id = `evt_${++eventSeq}`) {
  return JSON.stringify({
    event_id: id,
    event_type: type,
    occurred_at: iso(at),
    notification_id: `ntf_${eventSeq}`,
    data,
  });
}

function sendRaw(raw: string, signature: string | null = sign(raw)) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (signature !== null) headers["Paddle-Signature"] = signature;
  return call("/webhook/paddle", { method: "POST", headers, body: raw });
}

const send = (type: string, data: unknown, at = nowSecs(), id?: string) => sendRaw(rawEvent(type, data, at, id));

function sub(overrides: Record<string, unknown> = {}, opts: { plan?: "monthly" | "yearly"; ref?: string | null } = {}) {
  const yearly = opts.plan === "yearly";
  const ref = opts.ref === undefined ? R1 : opts.ref;
  return {
    id: SUB1,
    status: "active",
    customer_id: CUS1,
    custom_data: ref ? { kyra_ref: ref } : null,
    billing_cycle: { interval: yearly ? "year" : "month", frequency: 1 },
    current_billing_period: { starts_at: iso(NOW), ends_at: iso(NOW + (yearly ? 365 : 30) * DAY) },
    scheduled_change: null,
    items: [{ status: "active", quantity: 1, price: { id: yearly ? PRICE_Y : PRICE_M } }],
    ...overrides,
  };
}

function txn(overrides: Record<string, unknown> = {}, opts: { plan?: "monthly" | "yearly"; ref?: string | null } = {}) {
  const yearly = opts.plan === "yearly";
  const ref = opts.ref === undefined ? R1 : opts.ref;
  return {
    id: "txn_01hv8wptq8987qeep44cyrewp9",
    status: "completed",
    origin: "api",
    customer_id: CUS1,
    subscription_id: SUB1,
    custom_data: ref ? { kyra_ref: ref } : null,
    billing_period: { starts_at: iso(NOW), ends_at: iso(NOW + (yearly ? 365 : 30) * DAY) },
    items: [{ quantity: 1, price: { id: yearly ? PRICE_Y : PRICE_M } }],
    ...overrides,
  };
}

async function license(deviceId: string) {
  return (await call(`/license?device_id=${deviceId}`)).json();
}

async function checkout(deviceId = D1, email = EMAIL, plan?: string) {
  const res = await post("/checkout/create", { device_id: deviceId, email, ...(plan && { plan }) });
  expect(res.status).toBe(200);
  return res.json();
}

async function purchase(at = NOW, plan: "monthly" | "yearly" = "monthly") {
  await checkout(D1, EMAIL, plan);
  const completed = await send("transaction.completed", txn({}, { plan }), at);
  await send("subscription.activated", sub({}, { plan }), at);
  return completed;
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
    PADDLE_ENV: "sandbox",
    PADDLE_API_KEY: "pdl_sdbx_apikey_testkey",
    PADDLE_WEBHOOK_SECRET: SECRET,
    PADDLE_PRICE_ID_MONTHLY: PRICE_M,
    PADDLE_PRICE_ID_YEARLY: PRICE_Y,
    PADDLE_CLIENT_TOKEN: "test_clienttoken123",
    RESEND_API_KEY: "re_test",
    MAIL_FROM: "Kyra <hello@kyra.test>",
  };
  emails = [];
  customers = new Map();
  transactions = new Map();
  subscriptions = new Map();
  paddleCalls = [];
  txnCounter = 0;
  transactionResponse = null;
  paddle = vi.fn(async (url: string, init: RequestInit) => {
    const u = new URL(url);
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(init.body as string) : undefined;

    if (method === "POST" && u.pathname === "/customers") {
      const existing = customers.get(body.email);
      if (existing) {
        return Response.json(
          {
            error: {
              type: "request_error",
              code: "customer_already_exists",
              detail: `customer email conflicts with customer of id ${existing}`,
            },
          },
          { status: 409 }
        );
      }
      const id = `ctm_${String(customers.size + 1).padStart(26, "0")}`;
      customers.set(body.email, id);
      return Response.json({ data: { id, email: body.email, status: "active" } }, { status: 201 });
    }
    if (method === "GET" && u.pathname === "/customers") {
      const email = u.searchParams.get("email")!;
      const id = customers.get(email);
      return Response.json({ data: id ? [{ id, email }] : [] });
    }
    if (method === "POST" && u.pathname === "/transactions") {
      if (transactionResponse) return transactionResponse();
      const id = `txn_${String(++txnCounter).padStart(26, "0")}`;
      const t = {
        id,
        status: "draft",
        customer_id: body.customer_id,
        custom_data: body.custom_data,
        subscription_id: null,
        checkout: { url: `${body.checkout.url}?_ptxn=${id}` },
      };
      transactions.set(id, t);
      return Response.json({ data: t }, { status: 201 });
    }
    let m = /^\/transactions\/([^/]+)$/.exec(u.pathname);
    if (method === "GET" && m) {
      const t = transactions.get(m[1]);
      return t ? Response.json({ data: t }) : Response.json({ error: { code: "not_found" } }, { status: 404 });
    }
    m = /^\/subscriptions\/([^/]+)$/.exec(u.pathname);
    if (method === "GET" && m) {
      const s = subscriptions.get(m[1]);
      return s ? Response.json({ data: s }) : Response.json({ error: { code: "not_found" } }, { status: 404 });
    }
    m = /^\/customers\/([^/]+)\/portal-sessions$/.exec(u.pathname);
    if (method === "POST" && m) {
      return Response.json(
        {
          data: {
            id: "cpls_1",
            customer_id: m[1],
            urls: {
              general: { overview: PORTAL_OVERVIEW },
              subscriptions: (body.subscription_ids ?? []).map((id: string) => ({
                id,
                view_subscription: PORTAL_SUB,
                cancel_subscription: `${PORTAL_SUB}&c`,
                update_subscription_payment_method: `${PORTAL_SUB}&u`,
              })),
            },
          },
        },
        { status: 201 }
      );
    }
    throw new Error(`unexpected Paddle call ${method} ${url}`);
  });
  resend = vi.fn(async (_url: string, init: RequestInit) => {
    emails.push(JSON.parse(init.body as string));
    return Response.json({ id: "email_1" });
  });
  fetchMock = vi.fn(async (url: string, init: RequestInit) => {
    if (url.startsWith(SANDBOX_API) || url.startsWith("https://api.paddle.com")) {
      const u = new URL(url);
      const body = init?.body ? JSON.parse(init.body as string) : undefined;
      paddleCalls.push({ method: init?.method ?? "GET", path: u.pathname + u.search, body, headers: init.headers as Record<string, string> });
      return paddle(url, init);
    }
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

// Binds a device directly, for accounts whose subscription has lapsed (restore needs an active one).
function bindViaAccount(deviceId: string) {
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
  setTime(nowSecs() + 3601);
}

describe("POST /checkout/create", () => {
  it("creates a Paddle customer and transaction and returns its checkout link", async () => {
    const res = await post("/checkout/create", { device_id: D1, email: "  Jane+kyra@Gmail.com " });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toEqual({
      short_url: "https://worker.test/pay?_ptxn=txn_00000000000000000000000001",
      app_user_id: R1,
    });
    expect(body.short_url).not.toContain(D1);

    expect(paddleCalls.map((c) => `${c.method} ${c.path}`)).toEqual(["POST /customers", "POST /transactions"]);
    expect(paddleCalls[0].body).toEqual({ email: "jane+kyra@gmail.com" });
    expect(paddleCalls[1].body).toEqual({
      items: [{ price_id: PRICE_M, quantity: 1 }],
      customer_id: "ctm_00000000000000000000000001",
      custom_data: { kyra_ref: R1 },
      collection_mode: "automatic",
      checkout: { url: "https://worker.test/pay" },
    });
    expect(JSON.stringify(paddleCalls[1].body)).not.toContain(D1);
    const headers = paddleCalls[1].headers;
    expect(headers.Authorization).toBe("Bearer pdl_sdbx_apikey_testkey");
    expect(headers["Paddle-Version"]).toBe("1");
    expect(fetchMock.mock.calls[0][0]).toBe(`${SANDBOX_API}/customers`);

    expect(kv.json(`pending:${R1}`)).toEqual({
      device_id: D1,
      email: "jane+kyra@gmail.com",
      customer_id: "ctm_00000000000000000000000001",
      transaction_id: "txn_00000000000000000000000001",
      plan: "monthly",
    });
    expect(kv.store.get(`pending:${R1}`)!.ttl).toBe(7 * DAY);
  });

  it("uses the yearly price for plan yearly and monthly when asked explicitly", async () => {
    await checkout(D1, EMAIL, "yearly");
    expect(paddleCalls[1].body.items).toEqual([{ price_id: PRICE_Y, quantity: 1 }]);
    expect(kv.json(`pending:${R1}`).plan).toBe("yearly");

    await checkout(D1, EMAIL, "monthly");
    expect(paddleCalls.at(-1)!.body.items).toEqual([{ price_id: PRICE_M, quantity: 1 }]);
    expect(kv.json(`pending:${R1}`).plan).toBe("monthly");
  });

  it.each([["weekly"], ["Yearly"], [""], [12], [true], [{}]])("rejects plan %j with 400 invalid_plan", async (plan) => {
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL, plan });
    expect(res.status).toBe(400);
    expect((await res.json()).code).toBe("invalid_plan");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("treats a null plan as monthly", async () => {
    await checkout(D1, EMAIL);
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL, plan: null })).status).toBe(200);
    expect(paddleCalls.at(-1)!.body.items[0].price_id).toBe(PRICE_M);
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

  it("reuses an existing Paddle customer named in the 409 conflict", async () => {
    customers.set(EMAIL, CUS1);
    await checkout();
    expect(paddleCalls.map((c) => `${c.method} ${c.path}`)).toEqual(["POST /customers", "POST /transactions"]);
    expect(paddleCalls[1].body.customer_id).toBe(CUS1);
    expect(kv.json(`pending:${R1}`).customer_id).toBe(CUS1);
  });

  it("looks the customer up by email when the conflict does not name it", async () => {
    customers.set(EMAIL, CUS1);
    paddle.mockResolvedValueOnce(
      Response.json({ error: { code: "customer_already_exists", detail: "conflict" } }, { status: 409 })
    );
    await checkout();
    expect(paddleCalls.map((c) => `${c.method} ${c.path}`)).toEqual([
      "POST /customers",
      `GET /customers?email=${encodeURIComponent(EMAIL)}`,
      "POST /transactions",
    ]);
    expect(paddleCalls[2].body.customer_id).toBe(CUS1);
  });

  it("uses PADDLE_CHECKOUT_URL as the checkout page when set", async () => {
    env.PADDLE_CHECKOUT_URL = "https://kyra.app/pay";
    const { short_url } = await checkout();
    expect(paddleCalls[1].body.checkout).toEqual({ url: "https://kyra.app/pay" });
    expect(short_url).toBe("https://kyra.app/pay?_ptxn=txn_00000000000000000000000001");
  });

  it("talks to the production API when PADDLE_ENV is production", async () => {
    env.PADDLE_ENV = "production";
    await checkout();
    expect(fetchMock.mock.calls.map((c) => c[0])).toEqual([
      "https://api.paddle.com/customers",
      "https://api.paddle.com/transactions",
    ]);
  });

  it.each([
    ["no price id", () => (env.PADDLE_PRICE_ID_MONTHLY = "")],
    ["no API key", () => (env.PADDLE_API_KEY = "")],
    ["an http checkout page", () => (env.PADDLE_CHECKOUT_URL = "http://kyra.app/pay")],
    ["a customer error", () => paddle.mockResolvedValueOnce(Response.json({ error: { code: "forbidden", detail: "pdl_sdbx_apikey_testkey" } }, { status: 403 }))],
    ["a customer conflict of another kind", () => paddle.mockResolvedValueOnce(Response.json({ error: { code: "conflict", detail: CUS1 } }, { status: 409 }))],
    ["a transaction error", () => (transactionResponse = () => Response.json({ error: { detail: "pdl_sdbx_apikey_testkey" } }, { status: 500 }))],
    ["a network failure", () => paddle.mockRejectedValueOnce(new TypeError("fetch failed"))],
    ["a transaction without checkout URL", () => (transactionResponse = () => Response.json({ data: { id: "txn_x", checkout: null } }, { status: 201 }))],
    ["a transaction with an http checkout URL", () => (transactionResponse = () => Response.json({ data: { id: "txn_x", checkout: { url: "http://evil.test/pay?_ptxn=txn_x" } } }, { status: 201 }))],
    ["invalid JSON from Paddle", () => (transactionResponse = () => new Response("<html>", { status: 200 }))],
  ])("is a 502 with %s, without leaking provider details", async (_name, arrange) => {
    arrange();
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect(res.status).toBe(502);
    const body = JSON.stringify(await res.json());
    expect(body).toContain("payment_provider_error");
    expect(body).not.toContain("pdl_");
    expect([...kv.store.keys()].some((k) => k.startsWith("pending:"))).toBe(false);
  });

  it("refuses a Mac that is already subscribed and reuses the account for a lapsed one", async () => {
    await purchase();
    const again = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect(again.status).toBe(409);
    expect((await again.json()).code).toBe("already_active");

    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 100);
    const D2 = dev(2);
    bindViaAccount(D2);
    paddleCalls = [];
    const body = await checkout(D2);
    expect(body.app_user_id).toBe(R1);
    // The account's customer is reused without another lookup.
    expect(paddleCalls.map((c) => c.path)).toEqual(["/transactions"]);
    expect(paddleCalls[0].body.customer_id).toBe(CUS1);
  });

  it("rate limits checkout creation per IP", async () => {
    for (let i = 0; i < 10; i++) {
      expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(200);
    }
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL })).status).toBe(429);
    expect((await post("/checkout/create", { device_id: D1, email: EMAIL }, { ip: "198.51.100.9" })).status).toBe(200);
  });
});

describe("webhook signature", () => {
  const body = () => rawEvent("transaction.completed", txn());

  it("accepts a valid signature", async () => {
    await checkout();
    const raw = body();
    const res = await sendRaw(raw, sign(raw));
    expect(res.status).toBe(200);
    expect((await res.json()).action).toBe("activated");
  });

  it.each([
    ["missing", () => null],
    ["wrong secret", (raw: string) => sign(raw, nowSecs(), "pdl_ntfset_other")],
    ["tampered body", (raw: string) => sign(raw.replace("completed", "Completed"))],
    ["stale timestamp", (raw: string) => sign(raw, nowSecs() - 301)],
    ["future timestamp", (raw: string) => sign(raw, nowSecs() + 301)],
    ["timestamp swapped", (raw: string) => sign(raw).replace(/^ts=\d+/, `ts=${nowSecs() - 1}`)],
    ["no h1", () => `ts=${nowSecs()}`],
    ["no ts", (raw: string) => sign(raw).replace(/^ts=\d+;/, "")],
    ["garbage", () => "nonsense"],
    ["truncated h1", (raw: string) => sign(raw).slice(0, -2)],
  ])("rejects a %s signature without writing", async (_name, signer) => {
    await checkout();
    const before = new Map(kv.store);
    const raw = body();
    const res = await sendRaw(raw, signer(raw));
    expect(res.status).toBe(401);
    expect(await res.json()).toEqual({ error: "Unauthorized" });
    expect(kv.store).toEqual(before);
  });

  it("accepts a timestamp within five minutes", async () => {
    await checkout();
    const raw = body();
    expect((await sendRaw(raw, sign(raw, nowSecs() - 299))).status).toBe(200);
  });

  it("accepts any matching h1 while a secret rotates", async () => {
    await checkout();
    const raw = body();
    const ts = nowSecs();
    const old = createHmac("sha256", "pdl_ntfset_old").update(`${ts}:${raw}`).digest("hex");
    const res = await sendRaw(raw, `ts=${ts};h1=${old};${sign(raw, ts).split(";")[1]}`);
    expect(res.status).toBe(200);
  });

  it("rejects everything when no webhook secret is configured", async () => {
    env.PADDLE_WEBHOOK_SECRET = "";
    const raw = body();
    expect((await sendRaw(raw, sign(raw, nowSecs(), ""))).status).toBe(401);
  });

  it("answers 400 for a signed malformed body and skips bodies without data", async () => {
    expect((await sendRaw("{nope")).status).toBe(400);
    expect(await (await sendRaw("{}")).json()).toEqual({ ok: true, message: "No event, skipped" });
  });
});

describe("webhook lifecycle", () => {
  it("transaction.completed writes the license, the account and all indexes", async () => {
    const res = await purchase();
    expect(await res.json()).toEqual({ ok: true, action: "activated", expires: NOW + 30 * DAY });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(kv.json(`license:${D1}`)).toEqual({ active: true, expires: NOW + 30 * DAY, ref: R1, paddle_env: "sandbox" });
    expect(kv.json(`account:${EMAIL}`)).toEqual({
      email: EMAIL,
      ref: R1,
      customer_id: CUS1,
      subscription_id: SUB1,
      plan: "monthly",
      status: "active",
      current_end: NOW + 30 * DAY,
      grace_end: null,
      cancel_at_period_end: false,
      management_url: null,
      devices: [{ device_id: D1, bound_at: NOW }],
      last_event_at: NOW * 1000,
      paddle_env: "sandbox",
    });
    expect(await kv.get(`device:${D1}`)).toBe(EMAIL);
    expect(await kv.get(`pdlref:${R1}`)).toBe(EMAIL);
    expect(await kv.get(`pdlsub:${SUB1}`)).toBe(EMAIL);
    expect(await kv.get(`pdlcus:${CUS1}`)).toBe(EMAIL);
  });

  it("binds the purchasing Mac when subscription.created arrives first", async () => {
    await checkout();
    const res = await send("subscription.created", sub());
    expect(await res.json()).toEqual({ ok: true, action: "activated", expires: NOW + 30 * DAY });
    await send("transaction.completed", txn());
    expect(kv.json(`account:${EMAIL}`).devices).toEqual([{ device_id: D1, bound_at: NOW }]);
    expect((await license(D1)).active).toBe(true);
  });

  it("a yearly purchase grants about a year and records the plan", async () => {
    const res = await purchase(NOW, "yearly");
    expect(await res.json()).toEqual({ ok: true, action: "activated", expires: NOW + 365 * DAY });
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 365 * DAY });
    expect(kv.store.get(`license:${D1}`)!.ttl).toBeGreaterThanOrEqual(365 * DAY);
    expect(kv.json(`account:${EMAIL}`).plan).toBe("yearly");
    expect((await (await call(`/account?device_id=${D1}`)).json()).plan).toBe("yearly");
  });

  it("grants only for the configured prices and ignores events for any other price", async () => {
    await checkout();
    const other = [{ price: { id: "pri_other", billing_cycle: { interval: "year", frequency: 1 } } }];
    for (const [type, data] of [
      ["transaction.completed", txn({ items: other })],
      ["subscription.created", sub({ items: other })],
      ["subscription.activated", sub({ items: [] })],
    ] as const) {
      const res = await send(type, data);
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({ ok: true, message: "Unknown price, ignored" });
    }
    expect(kv.store.has(`account:${EMAIL}`)).toBe(false);
    expect(await license(D1)).toEqual({ active: false, expires: null });

    // A configured price, even next to another item, grants; the plan is the price that matched.
    await send("transaction.completed", txn({ items: [...other, { price: { id: PRICE_Y } }], billing_period: { starts_at: iso(NOW), ends_at: iso(NOW + 365 * DAY) } }));
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "active", plan: "yearly", current_end: NOW + 365 * DAY });
    expect((await license(D1)).active).toBe(true);

    // Renewals or extensions for another price never extend access.
    await send("transaction.completed", txn({ origin: "subscription_recurring", items: other, billing_period: { starts_at: iso(NOW), ends_at: iso(NOW + 900 * DAY) } }), NOW + 10);
    expect(kv.json(`account:${EMAIL}`).current_end).toBe(NOW + 365 * DAY);
  });

  it("the license fallback ignores a subscription for another price", async () => {
    await purchase();
    subscriptions.set(SUB1, sub({ items: [{ price: { id: "pri_other" } }], current_billing_period: { starts_at: iso(NOW + 30 * DAY), ends_at: iso(NOW + 60 * DAY) } }));
    setTime(NOW + 30 * DAY + 5);
    expect((await license(D1)).active).toBe(false);
    expect(paddleCalls.map((c) => c.path)).toContain(`/subscriptions/${SUB1}`);
  });

  it("skips subscriptions it cannot tie to a checkout", async () => {
    const res = await send("transaction.completed", txn());
    expect(await res.json()).toEqual({ ok: true, message: "Unknown subscription, skipped" });
    await send("subscription.created", sub({}, { ref: null }));
    expect(kv.store.size).toBe(2); // only the two seen event ids
    expect([...kv.store.keys()].every((k) => k.startsWith("pdlevt:"))).toBe(true);
  });

  it("renewal through subscription.updated extends every bound device", async () => {
    await purchase();
    const D2 = dev(2);
    await bindViaRestore(D2);
    const next = { starts_at: iso(NOW + 30 * DAY), ends_at: iso(NOW + 60 * DAY) };
    await send("transaction.completed", txn({ origin: "subscription_recurring", billing_period: next }), NOW + 30 * DAY);
    await send("subscription.updated", sub({ current_billing_period: next }), NOW + 30 * DAY);
    for (const d of [D1, D2]) expect(kv.json(`license:${d}`).expires).toBe(NOW + 60 * DAY);
    expect(kv.json(`account:${EMAIL}`).current_end).toBe(NOW + 60 * DAY);
  });

  it("a scheduled cancel keeps access until the period ends, removing it resumes renewal", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    const cancel = sub({ scheduled_change: { action: "cancel", effective_at: iso(end), resume_at: null } });
    const res = await send("subscription.updated", cancel, NOW + 10);
    expect(await res.json()).toEqual({ ok: true, action: "active_until_period_end", expires: end });
    expect(await license(D1)).toEqual({ active: true, expires: end });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "cancelled", cancel_at_period_end: true });

    await send("subscription.updated", sub(), NOW + 20);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "active", cancel_at_period_end: false });

    await send("subscription.updated", cancel, NOW + 30);
    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), end);
    setTime(end + 1);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "expired", cancel_at_period_end: false });
  });

  it("subscription.canceled deactivates", async () => {
    await purchase();
    const res = await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 100);
    expect(await res.json()).toEqual({ ok: true, action: "deactivated" });
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`).status).toBe("expired");
  });

  it("past_due keeps access for a bounded grace period that repeats cannot extend", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    const unpaid = { starts_at: iso(end), ends_at: iso(end + 30 * DAY) };
    setTime(end + 60);
    const res = await send("subscription.past_due", sub({ status: "past_due", current_billing_period: unpaid }));
    expect(await res.json()).toEqual({ ok: true, action: "grace", expires: end + 7 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "billing_issue", current_end: end, grace_end: end + 7 * DAY });

    setTime(end + 3 * DAY);
    await send("subscription.updated", sub({ status: "past_due", current_billing_period: unpaid }));
    expect(kv.json(`account:${EMAIL}`).grace_end).toBe(end + 7 * DAY);
    expect((await license(D1)).active).toBe(true);

    // Recovery: the retried payment completes.
    await send("transaction.completed", txn({ origin: "subscription_recurring", billing_period: unpaid }));
    await send("subscription.updated", sub({ current_billing_period: unpaid }));
    expect(await license(D1)).toEqual({ active: true, expires: end + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "active", grace_end: null });
  });

  it("past_due without recovery ends with the grace period and then the cancellation", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    setTime(end);
    await send("subscription.past_due", sub({ status: "past_due", current_billing_period: { starts_at: iso(end), ends_at: iso(end + 30 * DAY) } }));
    setTime(end + 7 * DAY + 1);
    expect((await license(D1)).active).toBe(false);
    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }));
    expect(kv.json(`account:${EMAIL}`).status).toBe("expired");
  });

  it("paused deactivates, a scheduled pause keeps the paid period, resumed reactivates", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    await send("subscription.updated", sub({ scheduled_change: { action: "pause", effective_at: iso(end), resume_at: null } }), NOW + 10);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "cancelled", current_end: end });
    expect((await license(D1)).active).toBe(true);

    await send("subscription.paused", sub({ status: "paused", current_billing_period: null }), end);
    expect(await license(D1)).toEqual({ active: false, expires: null });

    const back = { starts_at: iso(end + 5 * DAY), ends_at: iso(end + 35 * DAY) };
    setTime(end + 5 * DAY);
    const res = await send("subscription.resumed", sub({ current_billing_period: back }));
    expect(await res.json()).toEqual({ ok: true, action: "activated", expires: end + 35 * DAY });
  });

  it("trialing counts as active", async () => {
    await checkout();
    await send("subscription.trialing", sub({ status: "trialing" }));
    expect(kv.json(`account:${EMAIL}`).status).toBe("active");
  });

  it("an approved full refund ends access immediately and survives the cancellation", async () => {
    await purchase();
    const adj = { id: "adj_1", action: "refund", type: "full", status: "pending_approval", customer_id: CUS1, subscription_id: SUB1, transaction_id: "txn_1" };
    expect(await (await send("adjustment.created", adj, NOW + 10)).json()).toEqual({ ok: true, message: "Adjustment ignored" });
    expect((await license(D1)).active).toBe(true);

    const res = await send("adjustment.updated", { ...adj, status: "approved" }, NOW + 20);
    expect(await res.json()).toEqual({ ok: true, action: "deactivated" });
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");

    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 30);
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");
    expect((await (await call(`/account?device_id=${D1}`)).json()).status).toBe("refunded");
  });

  it("ignores partial and rejected refunds and refunds for other subscriptions; chargebacks end access", async () => {
    await purchase();
    const base = { id: "adj_1", action: "refund", type: "full", status: "approved", customer_id: CUS1, subscription_id: SUB1 };
    for (const adj of [{ ...base, type: "partial" }, { ...base, status: "rejected" }, { ...base, action: "credit" }]) {
      expect(await (await send("adjustment.updated", adj, NOW + 10)).json()).toEqual({ ok: true, message: "Adjustment ignored" });
    }
    expect(await (await send("adjustment.updated", { ...base, subscription_id: SUB2 }, NOW + 10)).json()).toEqual({
      ok: true,
      message: "Unknown subscription, skipped",
    });
    expect((await license(D1)).active).toBe(true);
    await send("adjustment.created", { ...base, action: "chargeback" }, NOW + 20);
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");
  });

  it("a new payment after a refund reactivates", async () => {
    await purchase();
    await send("adjustment.updated", { action: "refund", type: "full", status: "approved", subscription_id: SUB1 }, NOW + 10);
    await send("subscription.updated", sub(), NOW + 20);
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");
    await send("transaction.completed", txn({ origin: "subscription_recurring" }), NOW + 30);
    expect(kv.json(`account:${EMAIL}`).status).toBe("active");
  });

  it("ignores stale and out-of-order events", async () => {
    await purchase(NOW + 100);
    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 300);
    const stale = await send("subscription.updated", sub(), NOW + 200);
    expect(await stale.json()).toEqual({ ok: true, message: "Stale event ignored" });
    expect(kv.json(`account:${EMAIL}`).status).toBe("expired");
    expect((await license(D1)).active).toBe(false);
  });

  it("ignores redelivered event ids", async () => {
    await purchase();
    await send("subscription.updated", sub({ scheduled_change: { action: "cancel", effective_at: iso(NOW + 30 * DAY) } }), NOW + 10, "evt_dup");
    const before = kv.json(`account:${EMAIL}`);
    const again = await send("subscription.updated", sub(), NOW + 20, "evt_dup");
    expect(await again.json()).toEqual({ ok: true, message: "Duplicate event ignored" });
    expect(kv.json(`account:${EMAIL}`)).toEqual(before);
    expect(kv.store.get("pdlevt:evt_dup")!.ttl).toBe(7 * DAY);
  });

  it("is idempotent for repeated snapshots with new event ids", async () => {
    await purchase();
    const before = kv.json(`account:${EMAIL}`);
    await send("transaction.completed", txn());
    await send("subscription.activated", sub());
    expect(kv.json(`account:${EMAIL}`)).toEqual(before);
  });

  it("ignores unrelated events, non-subscription transactions and one-off charges", async () => {
    await purchase();
    for (const type of ["transaction.created", "transaction.paid", "customer.created", "subscription.imported"]) {
      expect(await (await send(type, sub())).json()).toEqual({ ok: true, message: "Event ignored" });
    }
    for (const t of [txn({ subscription_id: null }), txn({ billing_period: null }), txn({ origin: "subscription_charge", billing_period: { starts_at: iso(NOW), ends_at: iso(NOW + 400 * DAY) } })]) {
      expect(await (await send("transaction.completed", t)).json()).toEqual({ ok: true, message: "Not a subscription event, skipped" });
    }
    expect(kv.json(`account:${EMAIL}`).current_end).toBe(NOW + 30 * DAY);
  });

  it("does not re-bind an evicted purchase device on renewal", async () => {
    await purchase();
    for (const n of [2, 3, 4]) await bindViaRestore(dev(n));
    expect(kv.json(`account:${EMAIL}`).devices.map((d: any) => d.device_id)).not.toContain(D1);
    const next = { starts_at: iso(NOW + 30 * DAY), ends_at: iso(NOW + 60 * DAY) };
    await send("transaction.completed", txn({ origin: "subscription_recurring", billing_period: next }));
    await send("subscription.updated", sub({ current_billing_period: next }));
    expect(kv.json(`account:${EMAIL}`).devices).toHaveLength(3);
    expect(await license(D1)).toEqual({ active: false, expires: null });
  });

  it("a purchase from another Mac with the same email takes over the account", async () => {
    await purchase();
    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 100);
    const D2 = dev(2);
    const R2 = refOf(D2);
    await checkout(D2);
    const period = { starts_at: iso(NOW + 200), ends_at: iso(NOW + 40 * DAY) };
    await send("transaction.completed", txn({ subscription_id: SUB2, billing_period: period }, { ref: R2 }), NOW + 200);
    const account = kv.json(`account:${EMAIL}`);
    expect(account).toMatchObject({ ref: R2, subscription_id: SUB2, status: "active", current_end: NOW + 40 * DAY });
    expect(account.devices.map((d: any) => d.device_id)).toEqual([D1, D2]);
    expect((await license(D1)).active).toBe(true);
    expect(await kv.get(`pdlsub:${SUB2}`)).toBe(EMAIL);

    // Late events for the old subscription are ignored while the new one is live.
    const late = await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 300);
    expect(await late.json()).toEqual({ ok: true, message: "Superseded subscription ignored" });
    expect((await license(D2)).active).toBe(true);
  });
});

describe("GET /license Paddle fallback", () => {
  it("refreshes an expired KV license for a bound device, then caches for 10 minutes", async () => {
    await purchase();
    const later = { starts_at: iso(NOW + 30 * DAY), ends_at: iso(NOW + 60 * DAY) };
    subscriptions.set(SUB1, sub({ current_billing_period: later }));
    setTime(NOW + 30 * DAY + 5);
    paddleCalls = [];
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 60 * DAY });
    expect(paddleCalls.map((c) => `${c.method} ${c.path}`)).toEqual([`GET /subscriptions/${SUB1}`]);
    expect(kv.json(`license:${D1}`)).toEqual({ active: true, expires: NOW + 60 * DAY, ref: R1, paddle_env: "sandbox" });

    kv.store.delete(`license:${D1}`);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(paddleCalls).toHaveLength(1);
    setTime(NOW + 30 * DAY + 606);
    kv.store.delete(`pdlsync:${D1}`);
    expect((await license(D1)).active).toBe(true);
    expect(paddleCalls).toHaveLength(2);
  });

  it("does not call Paddle while the KV license is valid", async () => {
    await purchase();
    paddleCalls = [];
    expect((await license(D1)).active).toBe(true);
    expect(paddleCalls).toEqual([]);
  });

  it("covers a late webhook right after checkout using the pending transaction", async () => {
    const { short_url } = await checkout();
    const txnId = new URL(short_url).searchParams.get("_ptxn")!;
    expect(await license(D1)).toEqual({ active: false, expires: null }); // not paid yet

    transactions.get(txnId)!.status = "completed";
    transactions.get(txnId)!.subscription_id = SUB1;
    subscriptions.set(SUB1, sub());
    kv.store.delete(`pdlsync:${D1}`);
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(paddleCalls.slice(-2).map((c) => c.path)).toEqual([`/transactions/${txnId}`, `/subscriptions/${SUB1}`]);
  });

  it("ignores a pending transaction that does not carry this device's ref", async () => {
    const { short_url } = await checkout();
    const t = transactions.get(new URL(short_url).searchParams.get("_ptxn")!)!;
    Object.assign(t, { status: "completed", subscription_id: SUB1, custom_data: { kyra_ref: refOf(dev(7)) } });
    subscriptions.set(SUB1, sub());
    expect((await license(D1)).active).toBe(false);
  });

  it("reports inactive when Paddle shows no access or fails", async () => {
    await purchase();
    setTime(NOW + 31 * DAY);
    subscriptions.set(SUB1, sub({ status: "canceled", current_billing_period: null }));
    expect(await license(D1)).toEqual({ active: false, expires: NOW + 30 * DAY });
    for (const failure of [
      () => paddle.mockResolvedValueOnce(new Response("down", { status: 503 })),
      () => paddle.mockRejectedValueOnce(new TypeError("fetch failed")),
      () => subscriptions.delete(SUB1),
    ]) {
      kv.store.delete(`pdlsync:${D1}`);
      failure();
      expect((await license(D1)).active).toBe(false);
    }
  });

  it("grants the remaining grace period while past due", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    subscriptions.set(SUB1, sub({ status: "past_due", current_billing_period: { starts_at: iso(end), ends_at: iso(end + 30 * DAY) } }));
    setTime(end + DAY);
    expect(await license(D1)).toEqual({ active: true, expires: end + 7 * DAY });
    setTime(end + 8 * DAY);
    kv.store.delete(`pdlsync:${D1}`);
    kv.store.delete(`license:${D1}`);
    expect((await license(D1)).active).toBe(false);
  });

  it("never revives a device evicted from a known account", async () => {
    await purchase();
    for (const n of [2, 3, 4]) await bindViaRestore(dev(n));
    subscriptions.set(SUB1, sub({ current_billing_period: { starts_at: iso(NOW), ends_at: iso(NOW + 90 * DAY) } }));
    paddleCalls = [];
    expect((await license(D1)).active).toBe(false);
    expect(paddleCalls).toEqual([]);
  });

  it("does nothing for unknown devices without a checkout", async () => {
    expect(await license(dev(9))).toEqual({ active: false, expires: null });
    expect(paddleCalls).toEqual([]);
  });
});

describe("review regressions", () => {
  const canceled = () => sub({ status: "canceled", current_billing_period: null });
  const fullRefund = { id: "adj_1", action: "refund", type: "full", status: "approved", customer_id: CUS1, subscription_id: SUB1, transaction_id: "txn_1" };

  it("a refund is not undone by the license fallback while Paddle keeps the subscription active", async () => {
    await purchase();
    await send("adjustment.updated", fullRefund, NOW + 10);
    subscriptions.set(SUB1, sub());
    paddleCalls = [];
    expect(await license(D1)).toEqual({ active: false, expires: null });
    setTime(NOW + 700);
    expect(await license(D1)).toEqual({ active: false, expires: null });
    expect(paddleCalls).toEqual([]);
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");

    // Nor through a pending checkout of another Mac on the refunded account.
    const D2 = dev(2);
    const { short_url } = await checkout(D2);
    const t = transactions.get(new URL(short_url).searchParams.get("_ptxn")!)!;
    Object.assign(t, { status: "completed", subscription_id: SUB2 });
    subscriptions.set(SUB2, sub({ id: SUB2 }, { ref: refOf(D2) }));
    paddleCalls = [];
    expect(await license(D2)).toEqual({ active: false, expires: null });
    expect(paddleCalls.map((c) => c.path)).not.toContain(`/subscriptions/${SUB2}`);

    // A new payment still restores access through the webhook.
    await send("transaction.completed", txn({ origin: "subscription_recurring" }), NOW + 800);
    expect(kv.json(`account:${EMAIL}`).status).toBe("active");
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
  });

  it("sandbox records are absent once PADDLE_ENV is production", async () => {
    await purchase();
    expect(kv.json(`account:${EMAIL}`).paddle_env).toBe("sandbox");
    // Records written before the field existed count as sandbox.
    const legacy = kv.json(`license:${D1}`);
    delete legacy.paddle_env;
    kv.store.set(`license:${D1}`, { value: JSON.stringify(legacy) });
    expect((await license(D1)).active).toBe(true);

    env.PADDLE_ENV = "production";
    expect(await license(D1)).toEqual({ active: false, expires: null });
    const scored = await post("/jev/score", { device_id: D1, questions: ["q"] });
    expect(scored.status).toBe(403);
    expect((await call(`/account?device_id=${D1}`)).status).toBe(404);
    expect((await post("/account/manage", { device_id: D1 })).status).toBe(404);
    await post("/restore/start", { email: EMAIL });
    expect(resend).not.toHaveBeenCalled();
    // The email can buy for real; the new records are production ones.
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect(res.status).toBe(200);
    await send("transaction.completed", txn({ subscription_id: SUB2 }), NOW + 10);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ subscription_id: SUB2, paddle_env: "production", status: "active" });
    expect(kv.json(`license:${D1}`).paddle_env).toBe("production");
    expect((await license(D1)).active).toBe(true);

    env.PADDLE_ENV = "sandbox";
    expect((await call(`/account?device_id=${D1}`)).status).toBe(404);
  });

  it("a late event for a superseded subscription does not take the account back", async () => {
    await purchase();
    await send("subscription.canceled", canceled(), NOW + 100);
    await checkout(D1);
    const period = { starts_at: iso(NOW + 200), ends_at: iso(NOW + 40 * DAY) };
    await send("transaction.completed", txn({ subscription_id: SUB2, billing_period: period }), NOW + 200);
    await send("subscription.activated", sub({ id: SUB2, current_billing_period: period }), NOW + 200);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ subscription_id: SUB2, current_end: NOW + 40 * DAY });

    // A retried delivery of an old `active` snapshot for A.
    const late = await send("subscription.updated", sub(), NOW + 50);
    expect(await late.json()).toEqual({ ok: true, message: "Stale event ignored" });
    // Newer, but A pays no further ahead than B.
    const newer = await send("subscription.updated", sub(), NOW + 300);
    expect(await newer.json()).toEqual({ ok: true, message: "Superseded subscription ignored" });

    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ subscription_id: SUB2, status: "active", current_end: NOW + 40 * DAY });
    expect(await kv.get(`pdlsub:${SUB2}`)).toBe(EMAIL);
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 40 * DAY });
  });

  it("refuses checkout on a new Mac for an email that is still entitled", async () => {
    await purchase();
    const D2 = dev(2);
    const attempt = async (email = EMAIL) => {
      paddleCalls = [];
      const res = await post("/checkout/create", { device_id: D2, email }, { ip: `198.51.100.${++eventSeq % 250}` });
      return { status: res.status, code: (await res.json()).code };
    };
    expect(await attempt(" JANE@gmail.com ")).toEqual({ status: 409, code: "already_active" });
    expect(paddleCalls).toEqual([]);
    expect(kv.store.has(`pending:${refOf(D2)}`)).toBe(false);

    await send("subscription.updated", sub({ scheduled_change: { action: "cancel", effective_at: iso(NOW + 30 * DAY) } }), NOW + 10);
    expect(await attempt()).toEqual({ status: 409, code: "already_active" });

    setTime(NOW + 30 * DAY);
    await send("subscription.past_due", sub({ status: "past_due", current_billing_period: { starts_at: iso(NOW + 30 * DAY), ends_at: iso(NOW + 60 * DAY) } }));
    expect(await attempt()).toEqual({ status: 409, code: "already_active" });

    setTime(NOW + 38 * DAY);
    expect((await attempt()).status).toBe(200);
  });

  it("a chargeback Paddle wins restores access from the subscription", async () => {
    await purchase();
    const cb = { ...fullRefund, id: "adj_cb", action: "chargeback" };
    await send("adjustment.created", cb, NOW + 10);
    expect((await license(D1)).active).toBe(false);

    subscriptions.set(SUB1, sub());
    const res = await send("adjustment.created", { ...cb, id: "adj_cbr", action: "chargeback_reverse" }, NOW + 20);
    expect(await res.json()).toEqual({ ok: true, action: "activated", expires: NOW + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "active", current_end: NOW + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`).refunded_by).toBeUndefined();
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
  });

  it("a reversal never lifts a genuine refund, and follows Paddle when the subscription ended", async () => {
    await purchase();
    await send("adjustment.updated", fullRefund, NOW + 10);
    subscriptions.set(SUB1, sub());
    const res = await send("adjustment.created", { ...fullRefund, id: "adj_r", action: "chargeback_reverse" }, NOW + 20);
    expect(await res.json()).toEqual({ ok: true, message: "Reversal without matching refund, skipped" });
    expect(kv.json(`account:${EMAIL}`).status).toBe("refunded");

    await send("transaction.completed", txn({ origin: "subscription_recurring" }), NOW + 30);
    await send("adjustment.created", { ...fullRefund, id: "adj_cb", action: "chargeback" }, NOW + 40);
    subscriptions.set(SUB1, canceled());
    await send("adjustment.created", { ...fullRefund, id: "adj_cbr", action: "chargeback_reverse" }, NOW + 50);
    expect(kv.json(`account:${EMAIL}`).status).toBe("expired");
    expect((await license(D1)).active).toBe(false);
  });

  it("a withdrawn chargeback warning restores the stored period when Paddle is unreachable", async () => {
    await purchase();
    const warning = { ...fullRefund, id: "adj_w", action: "chargeback_warning" };
    await send("adjustment.created", warning, NOW + 10);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "refunded", refunded_by: "chargeback_warning" });

    paddle.mockRejectedValueOnce(new TypeError("fetch failed"));
    // Paddle marks the original warning `reversed` when it creates the reversal.
    await send("adjustment.updated", { ...warning, status: "reversed" }, NOW + 20);
    expect(kv.json(`account:${EMAIL}`).status).toBe("active");
    expect(await license(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
  });

  it("past-due grace counts from the known paid end when Paddle's period starts earlier", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    const unpaid = { starts_at: iso(NOW + 10 * DAY), ends_at: iso(NOW + 40 * DAY) };
    setTime(end - DAY);
    const res = await send("subscription.past_due", sub({ status: "past_due", current_billing_period: unpaid }));
    expect(await res.json()).toEqual({ ok: true, action: "grace", expires: end + 7 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "billing_issue", current_end: end, grace_end: end + 7 * DAY });
    setTime(end + 3 * DAY);
    await send("subscription.past_due", sub({ status: "past_due", current_billing_period: unpaid }));
    expect(kv.json(`account:${EMAIL}`).grace_end).toBe(end + 7 * DAY);
    expect((await license(D1)).active).toBe(true);
  });

  it("the license fallback counts past-due grace from the known paid end too", async () => {
    await purchase();
    const end = NOW + 30 * DAY;
    subscriptions.set(SUB1, sub({ status: "past_due", current_billing_period: { starts_at: iso(NOW + 10 * DAY), ends_at: iso(NOW + 40 * DAY) } }));
    setTime(end + DAY);
    expect(await license(D1)).toEqual({ active: true, expires: end + 7 * DAY });
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
    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 1);
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

  it("binds the device, writes its license and returns the customer reference", async () => {
    const res = await verify(lastCode());
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ active: true, expires: NOW + 30 * DAY, app_user_id: R1 });
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
    await send("subscription.canceled", sub({ status: "canceled", current_billing_period: null }), NOW + 1);
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
      plan: "monthly",
      current_end: NOW + 30 * DAY,
      cancel_at_period_end: false,
      devices_count: 1,
      management_url: null,
    });
  });

  it("is 404 for unknown devices and 400 for malformed ids", async () => {
    expect((await call(`/account?device_id=${dev(9)}`)).status).toBe(404);
    expect((await call("/account?device_id=abc")).status).toBe(400);
  });
});

describe("POST /account/manage", () => {
  it("returns an authenticated portal link for the subscription", async () => {
    await purchase();
    paddleCalls = [];
    const res = await post("/account/manage", { device_id: D1 });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ url: PORTAL_SUB });
    expect(paddleCalls.map((c) => `${c.method} ${c.path}`)).toEqual([`POST /customers/${CUS1}/portal-sessions`]);
    expect(paddleCalls[0].body).toEqual({ subscription_ids: [SUB1] });
    // Portal links are short-lived, so nothing is stored.
    expect((await (await call(`/account?device_id=${D1}`)).json()).management_url).toBeNull();
  });

  it("falls back to the portal overview", async () => {
    await purchase();
    paddle.mockResolvedValueOnce(
      Response.json({ data: { urls: { general: { overview: PORTAL_OVERVIEW }, subscriptions: [] } } }, { status: 201 })
    );
    expect(await (await post("/account/manage", { device_id: D1 })).json()).toEqual({ url: PORTAL_OVERVIEW });
  });

  it("needs a bound device with a Paddle customer", async () => {
    await purchase();
    expect((await post("/account/manage", { device_id: dev(9) })).status).toBe(404);
    expect((await post("/account/manage", { device_id: "x" })).status).toBe(400);
    const account = kv.json(`account:${EMAIL}`);
    kv.store.set(`account:${EMAIL}`, { value: JSON.stringify({ ...account, customer_id: null }) });
    const none = await post("/account/manage", { device_id: D1 });
    expect(none.status).toBe(409);
    expect((await none.json()).code).toBe("no_active_subscription");
  });

  it("maps Paddle failures to 502 without echoing them", async () => {
    await purchase();
    paddle.mockResolvedValueOnce(Response.json({ error: { detail: "pdl_sdbx_apikey_testkey invalid" } }, { status: 403 }));
    const res = await post("/account/manage", { device_id: D1 });
    expect(res.status).toBe(502);
    expect(JSON.stringify(await res.json())).not.toContain("pdl_");
  });

  it("rate limits per IP", async () => {
    await purchase();
    for (let i = 0; i < 20; i++) expect((await post("/account/manage", { device_id: D1 })).status).toBe(200);
    expect((await post("/account/manage", { device_id: D1 })).status).toBe(429);
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

describe("GET /pay", () => {
  it("serves the Paddle.js page for the sandbox with a nonce-locked CSP", async () => {
    const res = await call("/pay?_ptxn=txn_01hv8wptq8987qeep44cyrewp9");
    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("text/html; charset=utf-8");
    expect(res.headers.get("X-Frame-Options")).toBe("DENY");
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const csp = res.headers.get("Content-Security-Policy")!;
    const nonce = /'nonce-([0-9a-f]{32})'/.exec(csp)![1];
    expect(csp).toContain("frame-src https://buy.paddle.com https://sandbox-buy.paddle.com");
    expect(csp).toContain("frame-ancestors 'none'");
    expect(csp).not.toContain("unsafe-eval");

    const html = await res.text();
    expect(html).toContain('<script src="https://cdn.paddle.com/paddle/v2/paddle.js"></script>');
    expect(html).toContain(`<script nonce="${nonce}">`);
    expect(html).toContain('{"token":"test_clienttoken123","environment":"sandbox"}');
    expect(html).toContain('Paddle.Environment.set("sandbox")');
    expect(html).toContain("checkout.completed");
    expect(html).toContain("Payment complete \\u2014 you can return to Kyra.");
    // The transaction id is read by Paddle.js from the URL, never echoed by the worker.
    expect(html).not.toContain("txn_01hv8wptq8987qeep44cyrewp9");
  });

  it("uses a fresh nonce per response and the production environment when configured", async () => {
    env.PADDLE_ENV = "production";
    const a = (await call("/pay")).headers.get("Content-Security-Policy");
    const res = await call("/pay");
    expect(res.headers.get("Content-Security-Policy")).not.toBe(a);
    expect(await res.text()).toContain('"environment":"production"');
  });

  it("escapes what it interpolates", async () => {
    env.PADDLE_CLIENT_TOKEN = `</script><script>alert("x")</script>&\u2028x`;
    const html = await (await call("/pay")).text();
    expect(html).not.toContain("</script><script>alert");
    expect(html).toContain('"token":"\\u003c/script\\u003e\\u003cscript\\u003ealert(\\"x\\")\\u003c/script\\u003e\\u0026\\u2028x"');
  });

  it("is 503 without a client-side token", async () => {
    env.PADDLE_CLIENT_TOKEN = "";
    const res = await call("/pay");
    expect(res.status).toBe(503);
    expect(await res.text()).not.toContain("paddle.js");
  });
});

describe("mock mode", () => {
  beforeEach(() => {
    env.DEV_MOCK_PADDLE = "1";
  });

  const local = { host: "http://127.0.0.1:8787" };
  const localLicense = async (deviceId: string) => (await call(`/license?device_id=${deviceId}`, local)).json();
  const open = (url: string) => {
    const u = new URL(url, local.host);
    return call(u.pathname + u.search, local);
  };

  it("runs checkout, payment, restore and management locally without external calls", async () => {
    const checkoutRes = await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    const { short_url, app_user_id } = await checkoutRes.json();
    expect(short_url).toBe(`http://127.0.0.1:8787/dev/mock-pay?ref=${R1}&plan=monthly`);
    expect(app_user_id).toBe(R1);

    const paid = await open(short_url);
    expect(paid.status).toBe(200);
    const paidHtml = await paid.text();
    expect(paidHtml).toContain("transaction.completed: 200");
    expect(paidHtml).toContain("subscription.activated: 200");
    expect(await localLicense(D1)).toEqual({ active: true, expires: NOW + 30 * DAY });
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ plan: "monthly", status: "active" });

    await post("/restore/start", { email: EMAIL }, local);
    const logged = (console.log as any).mock.calls.map((c: any[]) => c[0]).join("\n");
    const code = /restore code for jane@gmail\.com: (\d{6})/.exec(logged)![1];
    expect((await post("/restore/verify", { email: EMAIL, code, device_id: dev(2) }, local)).status).toBe(200);

    const { url } = await (await post("/account/manage", { device_id: D1 }, local)).json();
    expect(url).toBe(`http://127.0.0.1:8787/dev/mock-manage?ref=${R1}`);
    const manage = await (await open(url)).text();
    for (const type of ["cancel", "resume", "renew", "past_due", "canceled", "refund"]) expect(manage).toContain(`type=${type}`);

    setTime(NOW + 10);
    await open(`/dev/mock-pay?ref=${R1}&type=cancel`);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "cancelled", cancel_at_period_end: true });
    expect((await localLicense(D1)).active).toBe(true);
    setTime(NOW + 20);
    await open(`/dev/mock-pay?ref=${R1}&type=resume`);
    expect(kv.json(`account:${EMAIL}`)).toMatchObject({ status: "active", cancel_at_period_end: false });
    setTime(NOW + 30);
    await open(`/dev/mock-pay?ref=${R1}&type=past_due`);
    expect(kv.json(`account:${EMAIL}`).status).toBe("billing_issue");
    setTime(NOW + 40);
    await open(`/dev/mock-pay?ref=${R1}&type=canceled`);
    expect((await localLicense(D1)).active).toBe(false);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("mock yearly checkout grants a year", async () => {
    const { short_url } = await (await post("/checkout/create", { device_id: D1, email: EMAIL, plan: "yearly" }, local)).json();
    expect(short_url).toContain("plan=yearly");
    await open(short_url);
    expect(await localLicense(D1)).toEqual({ active: true, expires: NOW + 365 * DAY });
    expect(kv.json(`account:${EMAIL}`).plan).toBe("yearly");
  });

  it("refuses unknown customers and actions", async () => {
    expect((await call(`/dev/mock-pay?ref=${R1}`, local)).status).toBe(404);
    await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    expect((await call(`/dev/mock-pay?ref=${R1}&type=nope`, local)).status).toBe(400);
    expect((await call(`/dev/mock-pay?ref=${R1}&type=cancel`, local)).status).toBe(409);
  });

  it("stays off for non-local hosts even with the flag set", async () => {
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL });
    expect((await res.json()).short_url).toMatch(/^https:\/\/worker\.test\/pay\?_ptxn=txn_/);
    expect((await call(`/dev/mock-pay?ref=${R1}`)).status).toBe(404);
    expect((await call(`/dev/mock-manage?ref=${R1}`)).status).toBe(404);
  });

  it("is off without the flag", async () => {
    delete env.DEV_MOCK_PADDLE;
    const res = await post("/checkout/create", { device_id: D1, email: EMAIL }, local);
    // Real sandbox checkout against `wrangler dev`: the worker's own /pay page on loopback.
    expect((await res.json()).short_url).toMatch(/^http:\/\/127\.0\.0\.1:8787\/pay\?_ptxn=txn_/);
    expect((await call(`/dev/mock-pay?ref=${R1}`, local)).status).toBe(404);
  });
});
