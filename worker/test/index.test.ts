import { createHmac } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker, { verifyRazorpaySignature } from "../src/index";

class MemoryKV {
  store = new Map<string, { value: string; ttl?: number }>();

  async get(key: string): Promise<string | null> {
    return this.store.get(key)?.value ?? null;
  }

  async put(
    key: string,
    value: string,
    opts?: { expirationTtl?: number }
  ): Promise<void> {
    const ttl = opts?.expirationTtl;
    if (ttl !== undefined && ttl < 60) {
      throw new Error(`Invalid expiration_ttl of ${ttl}`);
    }
    this.store.set(key, { value, ttl });
  }

  async delete(key: string): Promise<void> {
    this.store.delete(key);
  }
}

const SECRET = "whsec_test";
const NOW = 1_800_000_000;
const DAY = 86_400;

let kv: MemoryKV;
let env: any;
let upstream: ReturnType<typeof vi.fn>;

function call(path: string, init?: RequestInit): Promise<Response> {
  return worker.fetch(new Request(`https://worker.test${path}`, init), env);
}

function seedLicense(deviceId: string, active: boolean, expires: number) {
  kv.store.set(`license:${deviceId}`, {
    value: JSON.stringify({ active, expires, subscription_id: "sub_1" }),
  });
}

function score(deviceId: string, questions = ["q1", "q2"]) {
  return call("/jev/score", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ device_id: deviceId, questions }),
  });
}

function sign(body: string, secret = SECRET): string {
  return createHmac("sha256", secret).update(body).digest("hex");
}

function webhook(event: string, entity: Record<string, unknown>, sig?: string) {
  const body = JSON.stringify({ event, payload: { subscription: { entity } } });
  return call("/webhook/razorpay", {
    method: "POST",
    headers: { "X-Razorpay-Signature": sig ?? sign(body) },
    body,
  });
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW * 1000);
  kv = new MemoryKV();
  env = {
    LICENSES: kv,
    JEV_API_KEY: "jev-key",
    JEV_API_URL: "https://jev.upstream/score",
    RAZORPAY_WEBHOOK_SECRET: SECRET,
  };
  upstream = vi.fn(async () =>
    Response.json({ scores: [{ score: 80, confidence: 0.9 }, { score: 20, confidence: 0.5 }] })
  );
  vi.stubGlobal("fetch", upstream);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("GET /license", () => {
  it("reports an active license", async () => {
    seedLicense("dev1", true, NOW + 10 * DAY);
    const res = await call("/license?device_id=dev1");
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ active: true, expires: NOW + 10 * DAY });
  });

  it("reports an expired license as inactive with its expiry", async () => {
    seedLicense("dev1", true, NOW - 1);
    expect(await (await call("/license?device_id=dev1")).json()).toEqual({
      active: false,
      expires: NOW - 1,
    });
  });

  it("reports a deactivated license as inactive", async () => {
    seedLicense("dev1", false, NOW + DAY);
    expect((await (await call("/license?device_id=dev1")).json()).active).toBe(false);
  });

  it("reports a missing license as inactive with null expiry", async () => {
    expect(await (await call("/license?device_id=nobody")).json()).toEqual({
      active: false,
      expires: null,
    });
  });

  it("requires device_id", async () => {
    const res = await call("/license");
    expect(res.status).toBe(400);
    expect(await res.json()).toEqual({ error: "Missing device_id" });
  });
});

describe("POST /jev/score", () => {
  it("proxies to the upstream with the API key when licensed", async () => {
    seedLicense("dev1", true, NOW + DAY);
    const res = await score("dev1");
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      scores: [{ score: 80, confidence: 0.9 }, { score: 20, confidence: 0.5 }],
    });

    expect(upstream).toHaveBeenCalledTimes(1);
    const [url, init] = upstream.mock.calls[0];
    expect(url).toBe("https://jev.upstream/score");
    expect(init.headers.Authorization).toBe("Bearer jev-key");
    // The device id must not be forwarded upstream.
    expect(JSON.parse(init.body)).toEqual({ questions: ["q1", "q2"] });
  });

  it("returns 403 'No active license' without a license", async () => {
    const res = await score("nobody");
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: "No active license" });
    expect(upstream).not.toHaveBeenCalled();
  });

  it("returns 403 'License expired' for expired or inactive licenses", async () => {
    seedLicense("old", true, NOW - 1);
    seedLicense("off", false, NOW + DAY);
    for (const id of ["old", "off"]) {
      const res = await score(id);
      expect(res.status).toBe(403);
      expect(await res.json()).toEqual({ error: "License expired" });
    }
    expect(upstream).not.toHaveBeenCalled();
  });

  it("rejects missing device_id, empty questions and bad JSON with 400", async () => {
    seedLicense("dev1", true, NOW + DAY);
    const noDevice = await call("/jev/score", {
      method: "POST",
      body: JSON.stringify({ questions: ["q"] }),
    });
    expect(noDevice.status).toBe(400);
    expect((await score("dev1", [])).status).toBe(400);
    const bad = await call("/jev/score", { method: "POST", body: "{nope" });
    expect(bad.status).toBe(400);
    expect(upstream).not.toHaveBeenCalled();
  });

  it("allows 10 calls per hour then returns 429", async () => {
    seedLicense("dev1", true, NOW + 10 * DAY);
    for (let i = 0; i < 10; i++) {
      vi.setSystemTime((NOW + i * 350) * 1000);
      expect((await score("dev1")).status).toBe(200);
    }
    const limited = await score("dev1");
    expect(limited.status).toBe(429);
    expect(await limited.json()).toEqual({ error: "Rate limit exceeded (10/hour)" });
    expect(upstream).toHaveBeenCalledTimes(10);
  });

  it("keeps rate-limit TTLs valid near the end of the window", async () => {
    seedLicense("dev1", true, NOW + 10 * DAY);
    expect((await score("dev1")).status).toBe(200);
    vi.setSystemTime((NOW + 3590) * 1000);
    expect((await score("dev1")).status).toBe(200);
    expect(kv.store.get("ratelimit:dev1")!.ttl).toBeGreaterThanOrEqual(60);
  });

  it("resets the rate limit after the window", async () => {
    seedLicense("dev1", true, NOW + 10 * DAY);
    for (let i = 0; i < 10; i++) await score("dev1");
    expect((await score("dev1")).status).toBe(429);
    vi.setSystemTime((NOW + 3601) * 1000);
    expect((await score("dev1")).status).toBe(200);
  });

  it("rate limits per device", async () => {
    seedLicense("a", true, NOW + DAY);
    seedLicense("b", true, NOW + DAY);
    for (let i = 0; i < 10; i++) await score("a");
    expect((await score("a")).status).toBe(429);
    expect((await score("b")).status).toBe(200);
  });

  it("maps upstream errors to 502 with the upstream status", async () => {
    seedLicense("dev1", true, NOW + DAY);
    upstream.mockResolvedValueOnce(new Response("overloaded", { status: 503 }));
    const res = await score("dev1");
    expect(res.status).toBe(502);
    expect(await res.json()).toEqual({ error: "Jev API error: 503" });
  });

  it("maps upstream network failures and bad JSON to 502", async () => {
    seedLicense("dev1", true, NOW + DAY);
    upstream.mockRejectedValueOnce(new TypeError("fetch failed"));
    expect((await score("dev1")).status).toBe(502);
    upstream.mockResolvedValueOnce(new Response("<html>", { status: 200 }));
    expect((await score("dev1")).status).toBe(502);
  });
});

describe("POST /webhook/razorpay", () => {
  const entity = {
    id: "sub_123",
    status: "active",
    notes: { device_id: "dev1" },
    current_end: NOW + 30 * DAY,
  };

  it("activates a license on subscription.activated", async () => {
    const res = await webhook("subscription.activated", entity);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, action: "activated" });

    const stored = kv.store.get("license:dev1")!;
    expect(JSON.parse(stored.value)).toEqual({
      active: true,
      expires: NOW + 30 * DAY,
      subscription_id: "sub_123",
    });
    expect(stored.ttl).toBe(35 * DAY);

    expect(await (await call("/license?device_id=dev1")).json()).toEqual({
      active: true,
      expires: NOW + 30 * DAY,
    });
  });

  it("extends the license on subscription.charged", async () => {
    await webhook("subscription.activated", entity);
    await webhook("subscription.charged", { ...entity, current_end: NOW + 60 * DAY });
    expect(JSON.parse(kv.store.get("license:dev1")!.value).expires).toBe(NOW + 60 * DAY);
  });

  it("keeps annual licenses in KV until the period ends", async () => {
    await webhook("subscription.activated", { ...entity, current_end: NOW + 365 * DAY });
    expect(kv.store.get("license:dev1")!.ttl).toBeGreaterThanOrEqual(365 * DAY);
  });

  it("defaults the expiry when current_end is missing", async () => {
    const { current_end, ...noEnd } = entity;
    await webhook("subscription.activated", noEnd);
    expect(JSON.parse(kv.store.get("license:dev1")!.value).expires).toBe(NOW + 35 * DAY);
  });

  it("rejects an invalid signature without writing", async () => {
    for (const sig of ["", "deadbeef", sign("other body"), sign("x", "wrong-secret"), "z".repeat(64)]) {
      const res = await webhook("subscription.activated", entity, sig);
      expect(res.status).toBe(401);
      expect(await res.json()).toEqual({ error: "Invalid signature" });
    }
    expect(kv.store.size).toBe(0);
  });

  it("rejects a signature made with the right secret over a tampered body", async () => {
    const body = JSON.stringify({ event: "subscription.activated", payload: { subscription: { entity } } });
    const tampered = body.replace("dev1", "attacker");
    const res = await call("/webhook/razorpay", {
      method: "POST",
      headers: { "X-Razorpay-Signature": sign(body) },
      body: tampered,
    });
    expect(res.status).toBe(401);
    expect(kv.store.size).toBe(0);
  });

  it("removes the license on subscription.cancelled and subscription.halted", async () => {
    for (const ev of ["subscription.cancelled", "subscription.halted"]) {
      await webhook("subscription.activated", entity);
      expect(kv.store.has("license:dev1")).toBe(true);
      const res = await webhook(ev, entity);
      expect(await res.json()).toEqual({ ok: true, action: "deactivated" });
      expect(kv.store.has("license:dev1")).toBe(false);
      expect((await (await call("/license?device_id=dev1")).json()).active).toBe(false);
    }
  });

  it("skips events without a device_id in notes", async () => {
    for (const e of [{ ...entity, notes: {} }, { ...entity, notes: undefined }]) {
      const res = await webhook("subscription.activated", e);
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({ ok: true, message: "No device_id in notes, skipped" });
    }
    expect(kv.store.size).toBe(0);
  });

  it("skips signed payloads with no subscription entity", async () => {
    const body = JSON.stringify({ event: "payment.captured", payload: {} });
    const res = await call("/webhook/razorpay", {
      method: "POST",
      headers: { "X-Razorpay-Signature": sign(body) },
      body,
    });
    expect(res.status).toBe(200);
    expect(kv.store.size).toBe(0);
  });

  it("ignores unrelated events", async () => {
    const res = await webhook("subscription.updated", entity);
    expect(await res.json()).toEqual({ ok: true, message: "Event ignored" });
    expect(kv.store.size).toBe(0);
  });

  it("rejects signed but malformed JSON with 400", async () => {
    const body = "{not json";
    const res = await call("/webhook/razorpay", {
      method: "POST",
      headers: { "X-Razorpay-Signature": sign(body) },
      body,
    });
    expect(res.status).toBe(400);
  });
});

describe("verifyRazorpaySignature", () => {
  it("accepts upper-case hex and rejects an empty secret", async () => {
    const sig = sign("payload");
    expect(await verifyRazorpaySignature("payload", sig.toUpperCase(), SECRET)).toBe(true);
    expect(await verifyRazorpaySignature("payload", sign("payload", ""), "")).toBe(false);
  });
});

describe("routing", () => {
  it("answers CORS preflight", async () => {
    const res = await call("/jev/score", { method: "OPTIONS" });
    expect(res.status).toBe(204);
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
  });

  it.each([
    ["GET", "/jev/score"],
    ["POST", "/license"],
    ["GET", "/webhook/razorpay"],
    ["GET", "/"],
    ["POST", "/jev/score/extra"],
    ["DELETE", "/license"],
  ])("%s %s is 404", async (method, path) => {
    const res = await call(path, { method, body: method === "POST" ? "{}" : undefined });
    expect(res.status).toBe(404);
    expect(await res.json()).toEqual({ error: "Not found" });
  });
});
