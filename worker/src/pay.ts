// GET /pay: the page Paddle's checkout links point at (`/pay?_ptxn=txn_…`). Paddle.js opens the
// checkout for the transaction in the query string by itself once initialised.
import { randomHex } from "./crypto";
import type { Env } from "./env";
import { escapeHtml, scriptJson } from "./http";
import { paddleEnvironment } from "./paddle";

const PADDLE_JS = "https://cdn.paddle.com/paddle/v2/paddle.js";

// Hosts Paddle.js loads from: its CDN (scripts, styles), the checkout frame, its APIs, and the
// Retain snippet it injects for live accounts.
function contentSecurityPolicy(nonce: string): string {
  return [
    "default-src 'none'",
    `script-src 'nonce-${nonce}' https://cdn.paddle.com https://sandbox-cdn.paddle.com https://public.profitwell.com`,
    "style-src 'unsafe-inline' https://cdn.paddle.com https://sandbox-cdn.paddle.com",
    "font-src https://cdn.paddle.com https://sandbox-cdn.paddle.com",
    "img-src data: https://*.paddle.com https://*.profitwell.com",
    "frame-src https://buy.paddle.com https://sandbox-buy.paddle.com",
    "connect-src https://*.paddle.com https://*.profitwell.com",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join("; ");
}

const STYLE = `
:root{--bg:#f5f5f7;--card:#fff;--text:#1d1d1f;--muted:#6e6e73;--border:rgba(0,0,0,.08);--accent:#0a84ff;--ok:#30d158}
@media (prefers-color-scheme:dark){:root{--bg:#1c1c1e;--card:#2c2c2e;--text:#f5f5f7;--muted:#a1a1a6;--border:rgba(255,255,255,.1)}}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px 16px;background:var(--bg);color:var(--text);font:15px/1.5 -apple-system,BlinkMacSystemFont,"SF Pro Text","Helvetica Neue",Arial,sans-serif;-webkit-font-smoothing:antialiased}
main{width:100%;max-width:380px;padding:32px 28px;border-radius:16px;background:var(--card);border:1px solid var(--border);box-shadow:0 10px 30px rgba(0,0,0,.08);text-align:center}
.mark{width:44px;height:44px;margin:0 auto 16px;border-radius:12px;display:flex;align-items:center;justify-content:center;background:var(--accent);color:#fff;font-weight:700;font-size:20px}
.done .mark{background:var(--ok)}
h1{margin:0 0 6px;font-size:19px;font-weight:600;letter-spacing:-.01em}
p{margin:0;color:var(--muted)}
`;

function page(status: number, title: string, message: string, body = "", headers: Record<string, string> = {}): Response {
  const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex"><title>Pawtrol checkout</title><style>${STYLE}</style></head><body><main id="card"><div class="mark" id="mark" aria-hidden="true">K</div><h1 id="title">${escapeHtml(title)}</h1><p id="status">${escapeHtml(message)}</p></main>${body}</body></html>`;
  return new Response(html, {
    status,
    headers: {
      "Content-Type": "text/html; charset=utf-8",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      "X-Frame-Options": "DENY",
      "Referrer-Policy": "strict-origin-when-cross-origin",
      ...headers,
    },
  });
}

export async function handlePayPage(_request: Request, env: Env): Promise<Response> {
  const token = (env.PADDLE_CLIENT_TOKEN || "").trim();
  if (!token) {
    return page(503, "Checkout unavailable", "Payments are not configured yet. Please try again later.", "", {
      "Content-Security-Policy": "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'",
    });
  }

  const nonce = randomHex(16);
  const config = scriptJson({ token, environment: paddleEnvironment(env) });
  const script = `<script src="${PADDLE_JS}"></script><script nonce="${nonce}">(function(){
var cfg=${config};
var title=document.getElementById("title"),status=document.getElementById("status"),card=document.getElementById("card"),mark=document.getElementById("mark"),done=false;
function show(t,s){title.textContent=t;status.textContent=s;}
if(!window.Paddle){show("Checkout unavailable","Could not load the payment form. Check your connection and reload this page.");return;}
if(!new URLSearchParams(location.search).has("_ptxn")){show("Pawtrol","Start your subscription from Kyra to open the checkout.");}
if(cfg.environment==="sandbox"){Paddle.Environment.set("sandbox");}
var dark=window.matchMedia&&window.matchMedia("(prefers-color-scheme: dark)").matches;
Paddle.Initialize({token:cfg.token,checkout:{settings:{displayMode:"overlay",allowLogout:false,theme:dark?"dark":"light"}},eventCallback:function(e){
if(!e||!e.name)return;
if(e.name==="checkout.completed"){done=true;card.className="done";mark.textContent="\\u2713";show("Payment complete","Payment complete \\u2014 you can return to Kyra.");}
else if(e.name==="checkout.closed"&&!done){show("Checkout closed","Reopen the checkout from Kyra whenever you are ready.");}
}});
})();</script>`;
  return page(200, "Opening checkout…", "Loading the secure payment form from Paddle.", script, {
    "Content-Security-Policy": contentSecurityPolicy(nonce),
  });
}
