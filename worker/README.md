# Pawtrol worker

Cloudflare Worker behind Pawtrol ($0.99/month): licenses, the scoring proxy, RevenueCat
Billing subscriptions (hosted checkout through a Web Purchase Link, cards processed by
Stripe), email-code restore and account management. Everything is stored in the `LICENSES`
KV namespace.

## Endpoints

| Method | Path | Body / query | Response |
| --- | --- | --- | --- |
| GET | `/license` | `?device_id=` | `{active, expires}` (`expires` null when unknown) |
| POST | `/jev/score` | `{device_id, questions[]}` | upstream JSON; needs a license, 10/hour/device |
| POST | `/webhook/revenuecat` | RevenueCat event, `Authorization` header | `{ok, action \| message}` |
| POST | `/checkout/create` | `{device_id, email}` | `{short_url, app_user_id}` |
| POST | `/restore/start` | `{email}` | always `{ok: true}` |
| POST | `/restore/verify` | `{email, code, device_id}` | `{active, expires, app_user_id}` |
| GET | `/account` | `?device_id=` | `{email (masked), status, current_end, cancel_at_period_end, devices_count, management_url}` or 404 |
| POST | `/account/manage` | `{device_id}` | `{url}`: single-use link to the RevenueCat customer portal |
| POST | `/subscription/cancel` | any | always 410 `use_management_url` |

New endpoints return errors as `{error, code}`. `code` is one of `invalid_json`,
`invalid_email`, `invalid_device_id`, `invalid_code`, `code_expired`,
`too_many_attempts`, `rate_limited`, `subscription_inactive`, `not_found`,
`already_active`, `no_active_subscription`, `use_management_url`,
`payment_provider_error`, `internal_error`. The original three endpoints keep their
`{error}` shape. `device_id` must be 64 lowercase hex characters on the new endpoints.

`status` is one of `active`, `cancelled` (renewal off, access until `current_end`),
`billing_issue` (renewal failed, access until the grace period ends), `expired`,
`refunded`.

### Identity

- A Mac is identified by its random `device_id` (a bearer secret, see below).
- The RevenueCat customer (`app_user_id`) for a Mac's first purchase is
  `kyra-` + SHA-256(`pawtrol-rc:` + device_id). It is derived rather than the device id
  itself because it travels in the checkout URL (browser history, RevenueCat's logs).
- An account is keyed by the buyer's email and holds one `app_user_id` plus up to 3 bound
  devices. A bound Mac that buys again reuses the account's `app_user_id`.
- The worker always resolves the `app_user_id` from its own KV binding; clients never
  choose it. `restore/verify` returns it for information only.

### KV layout

| Key | Value | TTL |
| --- | --- | --- |
| `license:{device_id}` | `{active, expires, app_user_id}` | max(35 d, period + 5 d) |
| `account:{email}` | `{email, app_user_id, status, current_end, grace_end, cancel_at_period_end, management_url, devices: [{device_id, bound_at}], last_event_at}` | none |
| `device:{device_id}` | email (reverse index) | none |
| `rcuser:{app_user_id}` | email (reverse index) | none |
| `pending:{app_user_id}` | `{device_id, email}` from checkout | 7 d |
| `restore:{email}` | `{hash, salt, expires, attempts}` | 10 min |
| `rcsync:{device_id}` | last RevenueCat license refresh | 10 min |
| `ratelimit:{device_id}`, `rl:checkout:{ip}`, `rl:manage:{ip}`, `rl:restore-ip:{ip}`, `rl:restore-email:{email}` | fixed-window counters | 1 h |

Emails are trimmed and lower-cased before use as keys.

### Subscription rules

- Handled webhook events: `INITIAL_PURCHASE`, `RENEWAL`, `UNCANCELLATION`,
  `PRODUCT_CHANGE` (access until `expiration_at_ms`); `CANCELLATION` (access until the
  period ends, except `cancel_reason` `CUSTOMER_SUPPORT`, a refund, which ends it now, and
  `BILLING_ERROR`, which is the retry that accompanies `BILLING_ISSUE`); `BILLING_ISSUE`
  (access until `grace_period_expiration_at_ms`, or the period end without a grace
  period); `EXPIRATION` (access ends); `TRANSFER` (the account follows the purchases from
  `transferred_from` to `transferred_to[0]`). Everything else is acknowledged with 200 and
  ignored.
- `SANDBOX` events are ignored unless `REVENUECAT_ALLOW_SANDBOX=1`, so a leaked sandbox
  link cannot unlock Pawtrol with Stripe test cards. Set it only while testing.
- The email comes from the account already linked to the `app_user_id`, else the pending
  checkout, else the `$email` subscriber attribute. Events for customers with none of these
  are acknowledged and skipped.
- Every event updates the licenses of all devices bound to the account.
- Events older than the account's `last_event_at` (`event_timestamp_ms`, which RevenueCat
  reuses on retries) are ignored, so retries and out-of-order deliveries are harmless.
  Late non-granting events from a previous `app_user_id` never override a newer live one.
- The purchasing device (from the pending checkout) is bound only the first time a
  customer is seen, so a device evicted later is not re-added on renewal.
- An account holds at most 3 devices; binding a 4th evicts the oldest and deletes its
  license and reverse index entry.
- `GET /license` answers from KV. When the KV license is missing or expired it asks
  RevenueCat (`pawtrol` entitlement on the customer's subscriptions) at most once per
  device per 10 minutes, which covers a webhook that is late or lost, e.g. while the app
  polls right after checkout. It never does this for a device that is not bound to an
  account that already exists, so the device cap cannot be bypassed.
- Cancel, resume and card changes happen in RevenueCat's customer portal
  (`POST /account/manage`). RevenueCat also emails that portal link with every receipt.
  The REST API can cancel RevenueCat Billing subscriptions server-side
  (`POST /v2/projects/{project_id}/subscriptions/{id}/actions/cancel`) if an in-app cancel
  button is wanted again.

### Security notes

- Webhook: the `Authorization` header must equal `REVENUECAT_WEBHOOK_AUTH`, compared as
  SHA-256 digests in constant time. Nothing is written before the check. RevenueCat can
  additionally HMAC-sign deliveries (`X-RevenueCat-Webhook-Signature`); not used yet.
- Restore codes: 6 digits from `crypto.getRandomValues` (rejection-sampled), stored only
  as `SHA-256(salt:email:code)` with a random salt, compared in constant time, 10 minute
  expiry, 5 attempts then the code is destroyed, single use. `restore/start` always
  answers `{ok: true}`, counts its per-email limit for every address (so a 429 reveals
  nothing), and sends the email after responding so timing does not reveal accounts
  either.
- Account and manage are authorised by possession of a bound `device_id`. There is no
  password or session: anyone who learns a bound device id can read the masked account
  and open the customer portal (where RevenueCat shows the subscription and can cancel
  it). This keeps the app sign-up free. The trade-off only holds if the id is unguessable
  and private, so the app keeps it as a random 256-bit value in its data directory, never
  shows or logs it, and it is never sent to anyone but this worker.
- KV is eventually consistent and not transactional, so rate limits and the attempt
  counter can overshoot slightly under concurrent requests. With 3 codes/hour and 5
  attempts per code, brute force stays near 15 guesses/hour against 1,000,000 codes.
- Upstream errors are logged as status codes only; responses never echo provider bodies
  or secrets.

### RevenueCat references

What this worker relies on, as documented on 2026-10-01:

- Web Purchase Links: `https://pay.rev.cat/<token>/<url-encoded app_user_id>`, `?email=`
  presets a non-editable email; a production and a sandbox URL per link; the app_user_id
  is required (404 without it):
  https://www.revenuecat.com/docs/web/web-billing/web-purchase-links
- RevenueCat Billing (Stripe as gateway, you are merchant of record; customers based in
  India are not supported) and the engine comparison (RevenueCat Billing, Stripe Billing,
  Paddle Billing all work with Web Purchase Links):
  https://www.revenuecat.com/docs/web/web-billing/configuring-overview,
  https://www.revenuecat.com/docs/web/overview
- Lifecycle (renewal failure sends `CANCELLATION` + `BILLING_ISSUE`, grace periods, refunds
  send `CANCELLATION` with `CUSTOMER_SUPPORT`, uncancel sends `UNCANCELLATION`):
  https://www.revenuecat.com/docs/web/web-billing/subscription-lifecycle
- Customer portal (cancel, resume, update card, invoices):
  https://www.revenuecat.com/docs/web/web-billing/customer-portal
- Webhooks (Authorization header, 200 required, retries 5x, out-of-order delivery) and
  event fields (`event_timestamp_ms`, `expiration_at_ms`, `grace_period_expiration_at_ms`,
  `cancel_reason`, `transferred_from/to`, `subscriber_attributes`, `environment`):
  https://www.revenuecat.com/docs/integrations/webhooks,
  https://www.revenuecat.com/docs/integrations/webhooks/event-types-and-fields
- REST API v2 (Bearer v2 secret key; `GET /projects/{id}/customers/{id}/subscriptions`,
  `GET /projects/{id}/subscriptions/{id}/authenticated_management_url`, 480 requests/min
  for customer information): https://www.revenuecat.com/docs/api-v2,
  https://www.revenuecat.com/docs/api-v2/subscription,
  https://www.revenuecat.com/docs/api-v2/customer/resources

Not confirmed by the docs and isolated in `src/revenuecat.ts`: whether RevenueCat Billing
copies the checkout email into the `$email` attribute (the pending checkout covers it), and
the exact shape of the long-lived `management_url` for RevenueCat Billing subscriptions
(the single-use authenticated link is used for the button).

## Tests

```sh
npm install
npm test            # vitest, in-memory KV, mocked RevenueCat/Resend/JEV, fake clock
npm run typecheck   # tsc --noEmit
```

## Local end-to-end run

`.dev.vars` (gitignored, never deployed) points the worker at local mocks.

```sh
cd worker
cp .dev.vars.example .dev.vars      # once
npm run mock-jev                    # terminal 1: mock scorer on 127.0.0.1:8788
npx wrangler dev --ip 127.0.0.1 --port 8787   # terminal 2: worker + local KV
```

Point a debug build of the app at it (release builds ignore the variable):

```sh
cd ..                               # repo root
KYRA_WORKER_URL=http://127.0.0.1:8787 npm run tauri dev   # terminal 3
```

### Mock payments (`DEV_MOCK_REVENUECAT=1`)

With the flag in `.dev.vars` and requests addressed to `127.0.0.1`/`localhost`:

- `/checkout/create` returns `http://127.0.0.1:8787/dev/mock-pay?app_user_id=kyra-…`
  instead of the Web Purchase Link. Opening it sends a correctly authorised
  `INITIAL_PURCHASE` webhook through the real webhook handler.
- `/account/manage` returns `http://127.0.0.1:8787/dev/mock-manage?app_user_id=…`, a page
  with links that fire `CANCELLATION`, `UNCANCELLATION`, `RENEWAL`, `BILLING_ISSUE` and
  `EXPIRATION` webhooks.
- `/restore/start` prints the code in the `wrangler dev` output
  (`[dev] restore code for you@example.com: 123456`) instead of emailing it.
- `/license` never calls RevenueCat.

Mock mode needs both the flag and a loopback request host, so it cannot switch on for a
deployed worker (its requests arrive on the workers.dev or custom hostname). Never add
`DEV_MOCK_REVENUECAT` to `wrangler.toml` or as a secret.

A license can also be seeded from the command line (mock checkout + mock webhook):

```sh
npm run seed-license                          # reads ~/Library/Application Support/com.kyra.app/device_id.v2
npm run seed-license -- <device_id> --days 1  # explicit id / length
npm run seed-license -- --cancel              # revoke (sends EXPIRATION)
```

Mock scores are deterministic: >= 1 GB scores 90, >= 100 MB scores 55, else 20.

Rust client against the same local worker:

```sh
npm run seed-license -- e2e0000000000000000000000000000000000000000000000000000000000000
cd ../src-tauri && cargo test guardian -- --ignored
```

## Deploy checklist

### Cloudflare

```sh
cd worker
npx wrangler login
npx wrangler kv namespace create LICENSES     # paste the printed id into wrangler.toml
```

Pick a route: the default `kyra-guardian.<account>.workers.dev`, or a custom domain
(Workers & Pages > kyra-guardian > Settings > Domains & Routes > Add custom domain, e.g.
`api.kyra.app`, on a zone in the same account). Release builds of the app must point at
that URL.

### RevenueCat

1. Create a project, then add a **RevenueCat Billing** (Web Billing) app to it.
2. Attach Stripe to its billing config: connect your Stripe account (or start with the
   claimable sandbox and claim it before going live). Optionally enable Stripe Tax.
3. Product catalog: create a subscription product, 1 month, USD 0.99 (add other currencies
   if wanted). Create the entitlement with identifier **`pawtrol`** and attach the product.
   Create an offering (e.g. `pawtrol`) with one package holding the product.
4. Funnels > Purchase Links: create a Web Purchase Link for that offering. Set the success
   behaviour to the default success page (or a page that says "return to Kyra"). Copy the
   **production** URL template (`https://pay.rev.cat/<token>`), without the app user id.
   Keep the sandbox URL private.
5. Customize the customer emails and the customer portal (brand, support email).
6. Integrations > Webhooks: URL `https://<worker-host>/webhook/revenuecat`, Authorization
   header value `Bearer <long random string>`, all events, production (add sandbox only
   while testing).
7. Project settings > API keys: create a **v2 secret key** with the
   `customer_information:subscriptions:read` permission (nothing else is needed). Note the
   project id (`proj…`).

### Resend

Add and verify the sending domain (the SPF/DKIM DNS records Resend lists), create an API
key with sending access, and choose the from address on that domain, e.g.
`Kyra <hello@kyra.app>`.

### Secrets

```sh
npx wrangler secret put REVENUECAT_SECRET_API_KEY      # sk_… (v2)
npx wrangler secret put REVENUECAT_PROJECT_ID          # proj…
npx wrangler secret put REVENUECAT_WEBHOOK_AUTH        # exact header value, e.g. "Bearer …"
npx wrangler secret put REVENUECAT_WEB_PURCHASE_LINK   # https://pay.rev.cat/<token>
npx wrangler secret put RESEND_API_KEY
npx wrangler secret put MAIL_FROM
npx wrangler secret put JEV_API_KEY
# Testing only, remove before launch:
# npx wrangler secret put REVENUECAT_ALLOW_SANDBOX     # 1
```

`JEV_API_URL` is a plain var in `wrangler.toml`.

### Ship

```sh
npm test && npm run typecheck
npx wrangler deploy
```

Then, with `REVENUECAT_ALLOW_SANDBOX=1` and the sandbox purchase link as
`REVENUECAT_WEB_PURCHASE_LINK`, run one checkout with a Stripe test card, confirm the
webhook shows 200 in RevenueCat, `GET /license?device_id=…` reports active, and Manage
opens the portal. Switch both back to production values before release.
