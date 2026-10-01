// Seeds (or revokes) a license in a locally running `wrangler dev` started with
// DEV_MOCK_REVENUECAT=1, by going through mock checkout and a mock RevenueCat webhook,
// so the real checkout and webhook paths are exercised end to end.
//
//   node scripts/seed-license.mjs [device_id] [--days N] [--cancel] [--email you@example.com] [--url http://127.0.0.1:8787]
//
// device_id defaults to the one the debug app stored on this Mac.
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  return i === -1 ? undefined : args.splice(i, 2)[1];
};
const cancel = args.includes("--cancel") && args.splice(args.indexOf("--cancel"), 1);
const days = Number(flag("--days") ?? 30);
const email = flag("--email") ?? "dev@example.com";
const url = (flag("--url") ?? "http://127.0.0.1:8787").replace(/\/$/, "");

if (!/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(url)) {
  console.error(`Refusing to seed non-local worker: ${url}`);
  process.exit(1);
}

const deviceIdFile = join(homedir(), "Library/Application Support/com.kyra.app/device_id.v2");
const deviceId =
  args[0] ?? (existsSync(deviceIdFile) ? readFileSync(deviceIdFile, "utf8").trim() : "");
if (!/^[0-9a-f]{64}$/.test(deviceId)) {
  console.error("Need a 64-hex device_id. Open Pawtrol once in the debug app, or pass one.");
  process.exit(1);
}

let appUserId;
if (cancel) {
  const res = await fetch(`${url}/account/manage`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ device_id: deviceId }),
  });
  const body = await res.json();
  if (!res.ok) {
    console.error(`account/manage -> ${res.status} ${JSON.stringify(body)}`);
    process.exit(1);
  }
  appUserId = new URL(body.url).searchParams.get("app_user_id");
} else {
  const res = await fetch(`${url}/checkout/create`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ device_id: deviceId, email }),
  });
  const body = await res.json();
  if (!res.ok || !body.short_url?.startsWith(`${url}/dev/mock-pay`)) {
    console.error(`checkout -> ${res.status} ${JSON.stringify(body)} (is DEV_MOCK_REVENUECAT=1 in .dev.vars?)`);
    process.exit(1);
  }
  appUserId = body.app_user_id;
}

const type = cancel ? "EXPIRATION" : "INITIAL_PURCHASE";
const pay = await fetch(
  `${url}/dev/mock-pay?app_user_id=${encodeURIComponent(appUserId)}&type=${type}&days=${days}`
);
console.log(`${type} -> ${pay.status}`);
const lic = await fetch(`${url}/license?device_id=${encodeURIComponent(deviceId)}`);
console.log(`license(${deviceId.slice(0, 12)}…) -> ${await lic.text()}`);
process.exit(pay.ok ? 0 : 1);
