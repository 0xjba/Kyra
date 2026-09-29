// Seeds (or revokes) a license in a locally running `wrangler dev` by sending a
// correctly signed webhook, so the real webhook path is exercised end to end.
//
//   node scripts/seed-license.mjs [device_id] [--days N] [--cancel] [--url http://127.0.0.1:8787]
//
// device_id defaults to the one the debug app stored on this Mac.
import { createHmac } from "node:crypto";
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
const url = (flag("--url") ?? "http://127.0.0.1:8787").replace(/\/$/, "");

if (!/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(url)) {
  console.error(`Refusing to seed non-local worker: ${url}`);
  process.exit(1);
}

const deviceIdFile = join(homedir(), "Library/Application Support/com.kyra.app/device_id.v2");
const deviceId =
  args[0] ?? (existsSync(deviceIdFile) ? readFileSync(deviceIdFile, "utf8").trim() : "");
if (!deviceId) {
  console.error("No device_id given and none stored yet. Open Pawtrol once in the debug app, or pass one.");
  process.exit(1);
}

const devVars = new URL("../.dev.vars", import.meta.url);
const secret = existsSync(devVars)
  ? /^RAZORPAY_WEBHOOK_SECRET=(.*)$/m.exec(readFileSync(devVars, "utf8"))?.[1]?.trim()
  : undefined;
if (!secret) {
  console.error("RAZORPAY_WEBHOOK_SECRET missing: copy .dev.vars.example to .dev.vars first.");
  process.exit(1);
}

const now = Math.floor(Date.now() / 1000);
const body = JSON.stringify({
  event: cancel ? "subscription.cancelled" : "subscription.activated",
  payload: {
    subscription: {
      entity: {
        id: "sub_local_dev",
        status: cancel ? "cancelled" : "active",
        notes: { device_id: deviceId },
        current_end: now + days * 86400,
      },
    },
  },
});
const signature = createHmac("sha256", secret).update(body).digest("hex");

const res = await fetch(`${url}/webhook/razorpay`, {
  method: "POST",
  headers: { "Content-Type": "application/json", "X-Razorpay-Signature": signature },
  body,
});
console.log(`webhook -> ${res.status} ${await res.text()}`);
const lic = await fetch(`${url}/license?device_id=${encodeURIComponent(deviceId)}`);
console.log(`license(${deviceId.slice(0, 12)}…) -> ${await lic.text()}`);
process.exit(res.ok ? 0 : 1);
