import { handleCheckoutCreate } from "./checkout";
import type { Ctx, Env } from "./env";
import { corsHeaders, fail, legacyError } from "./http";
import { handleJevScore, handleLicenseCheck } from "./license";
import { handleMockPay, mockEnabled } from "./mock";
import { handleRestoreStart, handleRestoreVerify } from "./restore";
import { handleAccount, handleCancel } from "./subscription";
import { handleRazorpayWebhook } from "./webhook";

export { verifyRazorpaySignature } from "./crypto";
export type { Env } from "./env";

type Handler = (request: Request, env: Env, ctx?: Ctx) => Promise<Response>;

const routes: Record<string, Handler> = {
  "GET /license": handleLicenseCheck,
  "POST /jev/score": handleJevScore,
  "POST /webhook/razorpay": handleRazorpayWebhook,
  "POST /checkout/create": handleCheckoutCreate,
  "POST /restore/start": handleRestoreStart,
  "POST /restore/verify": handleRestoreVerify,
  "GET /account": handleAccount,
  "POST /subscription/cancel": handleCancel,
};

export default {
  async fetch(request: Request, env: Env, ctx?: Ctx): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders });
    }

    const { pathname } = new URL(request.url);
    const handler = routes[`${request.method} ${pathname}`];
    if (handler) {
      try {
        return await handler(request, env, ctx);
      } catch (err) {
        console.error(`unhandled error on ${pathname}: ${err instanceof Error ? err.name : "unknown"}`);
        return fail(500, "internal_error", "Internal error");
      }
    }

    if (pathname === "/dev/mock-pay" && request.method === "GET" && mockEnabled(env, request)) {
      return handleMockPay(request, env);
    }

    return legacyError("Not found", 404);
  },
};
