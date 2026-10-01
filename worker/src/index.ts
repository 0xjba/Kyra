import { handleCheckoutCreate } from "./checkout";
import type { Ctx, Env } from "./env";
import { corsHeaders, fail, legacyError } from "./http";
import { handleJevScore, handleLicenseCheck } from "./license";
import { handleMockManage, handleMockPay, mockEnabled } from "./mock";
import { handlePayPage } from "./pay";
import { handleRestoreStart, handleRestoreVerify } from "./restore";
import { handleAccount, handleCancelGone, handleManage } from "./subscription";
import { handlePaddleWebhook } from "./webhook";

export type { Env } from "./env";

type Handler = (request: Request, env: Env, ctx?: Ctx) => Promise<Response>;

const routes: Record<string, Handler> = {
  "GET /license": handleLicenseCheck,
  "POST /jev/score": handleJevScore,
  "POST /webhook/paddle": handlePaddleWebhook,
  "POST /checkout/create": handleCheckoutCreate,
  "POST /restore/start": handleRestoreStart,
  "POST /restore/verify": handleRestoreVerify,
  "GET /account": handleAccount,
  "POST /account/manage": handleManage,
  "POST /subscription/cancel": handleCancelGone,
  "GET /pay": handlePayPage,
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

    if (request.method === "GET" && mockEnabled(env, request)) {
      if (pathname === "/dev/mock-pay") return handleMockPay(request, env);
      if (pathname === "/dev/mock-manage") return handleMockManage(request);
    }

    return legacyError("Not found", 404);
  },
};
