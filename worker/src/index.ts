interface Env {
  LICENSES: KVNamespace;
  JEV_API_KEY: string;
  JEV_API_URL: string;
  RAZORPAY_WEBHOOK_SECRET: string;
}

interface LicenseRecord {
  active: boolean;
  expires: number;
  subscription_id: string;
}

interface JevScoreRequest {
  device_id: string;
  questions: string[];
}

interface JevScoreResult {
  category: string;
  score: number;
  confidence: number;
}

const RATE_LIMIT_WINDOW = 3600;
const RATE_LIMIT_MAX = 10;
const LICENSE_TTL_SECONDS = 35 * 24 * 60 * 60;

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type",
};

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json", ...corsHeaders },
  });
}

function errorResponse(message: string, status: number): Response {
  return jsonResponse({ error: message }, status);
}

async function checkRateLimit(
  env: Env,
  deviceId: string
): Promise<boolean> {
  const key = `ratelimit:${deviceId}`;
  const raw = await env.LICENSES.get(key);
  const now = Math.floor(Date.now() / 1000);

  if (!raw) {
    await env.LICENSES.put(
      key,
      JSON.stringify({ count: 1, window_start: now }),
      { expirationTtl: RATE_LIMIT_WINDOW }
    );
    return true;
  }

  const data = JSON.parse(raw) as { count: number; window_start: number };
  if (now - data.window_start > RATE_LIMIT_WINDOW) {
    await env.LICENSES.put(
      key,
      JSON.stringify({ count: 1, window_start: now }),
      { expirationTtl: RATE_LIMIT_WINDOW }
    );
    return true;
  }

  if (data.count >= RATE_LIMIT_MAX) {
    return false;
  }

  data.count++;
  await env.LICENSES.put(key, JSON.stringify(data), {
    expirationTtl: RATE_LIMIT_WINDOW - (now - data.window_start),
  });
  return true;
}

async function handleJevScore(
  request: Request,
  env: Env
): Promise<Response> {
  const body = (await request.json()) as JevScoreRequest;

  if (!body.device_id || !body.questions?.length) {
    return errorResponse("Missing device_id or questions", 400);
  }

  const licenseRaw = await env.LICENSES.get(`license:${body.device_id}`);
  if (!licenseRaw) {
    return errorResponse("No active license", 403);
  }
  const license = JSON.parse(licenseRaw) as LicenseRecord;
  if (!license.active || license.expires < Date.now() / 1000) {
    return errorResponse("License expired", 403);
  }

  const allowed = await checkRateLimit(env, body.device_id);
  if (!allowed) {
    return errorResponse("Rate limit exceeded (10/hour)", 429);
  }

  const jevResponse = await fetch(env.JEV_API_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${env.JEV_API_KEY}`,
    },
    body: JSON.stringify({ questions: body.questions }),
  });

  if (!jevResponse.ok) {
    const text = await jevResponse.text();
    return errorResponse(`Jev API error: ${jevResponse.status}`, 502);
  }

  const jevData = await jevResponse.json();
  return jsonResponse(jevData);
}

async function handleLicenseCheck(
  request: Request,
  env: Env
): Promise<Response> {
  const url = new URL(request.url);
  const deviceId = url.searchParams.get("device_id");

  if (!deviceId) {
    return errorResponse("Missing device_id", 400);
  }

  const raw = await env.LICENSES.get(`license:${deviceId}`);
  if (!raw) {
    return jsonResponse({ active: false, expires: null });
  }

  const license = JSON.parse(raw) as LicenseRecord;
  const now = Math.floor(Date.now() / 1000);

  if (!license.active || license.expires < now) {
    return jsonResponse({ active: false, expires: license.expires });
  }

  return jsonResponse({ active: true, expires: license.expires });
}

async function verifyRazorpaySignature(
  body: string,
  signature: string,
  secret: string
): Promise<boolean> {
  const encoder = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const signed = await crypto.subtle.sign("HMAC", key, encoder.encode(body));
  const expected = Array.from(new Uint8Array(signed))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return expected === signature;
}

async function handleRazorpayWebhook(
  request: Request,
  env: Env
): Promise<Response> {
  const body = await request.text();
  const signature = request.headers.get("X-Razorpay-Signature") || "";

  const valid = await verifyRazorpaySignature(
    body,
    signature,
    env.RAZORPAY_WEBHOOK_SECRET
  );
  if (!valid) {
    return errorResponse("Invalid signature", 401);
  }

  const event = JSON.parse(body) as {
    event: string;
    payload: {
      subscription?: {
        entity: {
          id: string;
          status: string;
          notes?: { device_id?: string };
          current_end?: number;
        };
      };
    };
  };

  const sub = event.payload.subscription?.entity;
  if (!sub?.notes?.device_id) {
    return jsonResponse({ ok: true, message: "No device_id in notes, skipped" });
  }

  const deviceId = sub.notes.device_id;
  const kvKey = `license:${deviceId}`;

  switch (event.event) {
    case "subscription.activated":
    case "subscription.charged": {
      const expires = sub.current_end || Math.floor(Date.now() / 1000) + LICENSE_TTL_SECONDS;
      const record: LicenseRecord = {
        active: true,
        expires,
        subscription_id: sub.id,
      };
      await env.LICENSES.put(kvKey, JSON.stringify(record), {
        expirationTtl: LICENSE_TTL_SECONDS,
      });
      return jsonResponse({ ok: true, action: "activated" });
    }

    case "subscription.cancelled":
    case "subscription.halted": {
      await env.LICENSES.delete(kvKey);
      return jsonResponse({ ok: true, action: "deactivated" });
    }

    default:
      return jsonResponse({ ok: true, message: "Event ignored" });
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders });
    }

    const url = new URL(request.url);

    if (url.pathname === "/jev/score" && request.method === "POST") {
      return handleJevScore(request, env);
    }

    if (url.pathname === "/license" && request.method === "GET") {
      return handleLicenseCheck(request, env);
    }

    if (url.pathname === "/webhook/razorpay" && request.method === "POST") {
      return handleRazorpayWebhook(request, env);
    }

    return errorResponse("Not found", 404);
  },
};
