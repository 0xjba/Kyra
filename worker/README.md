# Pawtrol worker

Cloudflare Worker behind Pawtrol ($0.99/month or $9.99/year): licenses, the scoring proxy,
Paddle Billing subscriptions (Paddle is the merchant of record: it runs the checkout, takes
the payment, handles tax and refunds), email-code restore and account management. Everything
is stored in the `LICENSES` KV namespace.

## Endpoints

| Method | Path | Body / query | Response |
| --- | --- | --- | --- |
| GET | `/license` | `?device_id=` | `{active, expires}` (`expires` null when unknown) |
| POST | `/jev/score` | `{device_id, questions[]}` | upstream JSON; needs a license, 10/hour/device |
| POST | `/webhook/paddle` | Paddle notification, `Paddle-Signature` header | `{ok, action \| message}` |
| POST | `/checkout/create` | `{device_id, email, plan?}` (`plan`: `monthly` default, or `yearly`) | `{short_url, app_user_id}`: Paddle checkout link and the customer reference |
| GET | `/pay` | `?_ptxn=txn_…` (added by Paddle) | HTML page that opens the Paddle checkout |
| POST | `/restore/start` | `{email}` | always `{ok: true}` |
| POST | `/restore/verify` | `{email, code, device_id}` | `{active, expires, app_user_id}` |
| GET | `/account` | `?device_id=` | `{email (masked), status, plan, current_end, cancel_at_period_end, devices_count, management_url}` or 404 |
| POST | `/account/manage` | `{device_id}` | `{url}`: short-lived, signed-in link to Paddle's customer portal |
| POST | `/subscription/cancel` | any | always 410 `use_management_url` |

New endpoints return errors as `{error, code}`. `code` is one of `invalid_json`,
`invalid_email`, `invalid_device_id`, `invalid_plan`, `invalid_code`, `code_expired`,
`too_many_attempts`, `rate_limited`, `subscription_inactive`, `not_found`,
`already_active`, `no_active_subscription`, `use_management_url`,
`payment_provider_error`, `internal_error`. The original three endpoints keep their
`{error}` shape. `device_id` must be 64 lowercase hex characters on the new endpoints.

`status` is one of `active`, `cancelled` (renewal off, access until `current_end`),
`billing_issue` (renewal payment failed, access through a 7 day grace period), `expired`,
`refunded`. `plan` is `monthly`, `yearly` or null (not known yet). `management_url` is
always null: Paddle's portal links are temporary and must not be stored, so the app asks
`/account/manage` for a fresh one.

### Identity

- A Mac is identified by its random `device_id` (a bearer secret, see below).
- A Mac's first purchase is tagged with the reference `kyra-` + SHA-256(`pawtrol-ref:` +
  device_id), stored as `custom_data.kyra_ref` on the Paddle transaction (Paddle copies it to
  the subscription and its renewals). It is derived rather than the device id itself because
  it is visible in Paddle's dashboard, exports and webhooks. The API returns it as
  `app_user_id`, for information only.
- An account is keyed by the buyer's email and holds one reference, the Paddle customer
  (`ctm_…`, one per email in Paddle), the current subscription (`sub_…`) and up to 3 bound
  devices. A bound Mac that buys again reuses the account's reference and customer.
- The worker resolves everything from its own KV; clients never choose a reference,
  customer or subscription.

### KV layout

| Key | Value | TTL |
| --- | --- | --- |
| `license:{device_id}` | `{active, expires, ref, paddle_env}` | max(35 d, period + 5 d) |
| `account:{email}` | `{email, ref, customer_id, subscription_id, plan, status, current_end, grace_end, cancel_at_period_end, management_url, devices: [{device_id, bound_at}], last_event_at, refunded_by?, paddle_env}` | none |
| `device:{device_id}` | email (reverse index) | none |
| `pdlref:{ref}`, `pdlsub:{subscription_id}`, `pdlcus:{customer_id}` | email (reverse indexes) | none |
| `pending:{ref}` | `{device_id, email, customer_id, transaction_id, plan}` from checkout | 7 d |
| `pdlevt:{event_id}` | processed Paddle event ids (dedupe) | 7 d |
| `restore:{email}` | `{hash, salt, expires, attempts}` | 10 min |
| `pdlsync:{device_id}` | last Paddle license refresh | 10 min |
| `ratelimit:{device_id}`, `rl:checkout:{ip}`, `rl:manage:{ip}`, `rl:restore-ip:{ip}`, `rl:restore-email:{email}` | fixed-window counters | 1 h |

Emails are trimmed and lower-cased before use as keys. Reverse indexes are hints: the
account must still hold the id, so stale entries are harmless.

`paddle_env` is the `PADDLE_ENV` (`sandbox` | `production`) the record was written under. A
license or account whose `paddle_env` differs from the worker's current `PADDLE_ENV` is treated
as absent everywhere (`/license`, the `/jev/score` license gate, restore, `/account`, manage,
checkout and webhooks), so sandbox purchases never unlock a production worker. Records without
the field were written before it existed and count as `sandbox`.

### Checkout

`POST /checkout/create` answers 409 `already_active` when the Mac's account, or the account of
the requested email, is still entitled (`active`, `cancelled` before `current_end`, or
`billing_issue` within its grace period): one email has one live subscription, and a second
Mac joins it through restore instead of buying again. Otherwise it creates (or, on Paddle's 409
`customer_already_exists`, reuses) the Paddle customer for the email, then creates a
transaction with the plan's price, that customer, `custom_data.kyra_ref` and `checkout.url` set to the checkout page. Paddle returns
the transaction's `checkout.url` (`<page>?_ptxn=txn_…`), which the app opens in the browser.
The page is `PADDLE_CHECKOUT_URL` when set, else this worker's own `/pay`. `/pay` loads
Paddle.js from Paddle's CDN, initialises it with `PADDLE_CLIENT_TOKEN` (sandbox environment
when `PADDLE_ENV=sandbox`), and Paddle.js opens the overlay checkout for `_ptxn` by itself.
The email is fixed (`allowLogout: false`). When Paddle.js reports `checkout.completed` the
page says "Payment complete — you can return to Kyra."; the license itself comes from the
webhook.

### Subscription rules

- Handled webhook events: `transaction.completed`, `subscription.created`,
  `subscription.activated`, `subscription.updated`, `subscription.canceled`,
  `subscription.past_due`, `subscription.paused`, `subscription.resumed`,
  `subscription.trialing`, `adjustment.created`, `adjustment.updated`. Everything else is
  acknowledged with 200 and ignored.
- Subscription events carry the whole subscription, so the state is derived from the
  snapshot, whatever the event name:
  - `active` / `trialing`: access until `current_billing_period.ends_at`.
  - `active` with `scheduled_change.action` `cancel` (or `pause`): `cancelled`, access until
    `scheduled_change.effective_at`.
  - `past_due`: Paddle has already moved the period on to the unpaid one, so access is paid
    up to the later of `current_billing_period.starts_at` and the `current_end` the account
    already knew was paid; `billing_issue` keeps access for 7 days after that while Paddle
    retries the payment. Repeated past-due events cannot extend it; a successful retry
    (`transaction.completed`) makes it `active` again.
  - `canceled` / `paused`: `expired`, access ends.
- `transaction.completed` for a subscription grants access until the transaction's
  `billing_period.ends_at` (first payment and renewals). One-off charges and payment-method
  updates (`origin` `subscription_charge`, `subscription_payment_method_change`) and
  transactions without a subscription are skipped.
- Prices: only events with an item whose `price.id` is `PADDLE_PRICE_ID_MONTHLY` or
  `PADDLE_PRICE_ID_YEARLY` grant or extend access, and `plan` is whichever of the two matched.
  Subscription and transaction events for any other price are acknowledged with 200 and
  ignored, and the license fallback grants nothing for them. Replacing a price id therefore
  stops recognising subscribers still on the old price.
- Refunds: an `adjustment.*` with `action` `refund`, `chargeback` or `chargeback_warning`,
  `type` `full` and `status` `approved` makes the account `refunded` (access ends now) and
  records the action as `refunded_by`. Live refunds start as `pending_approval` and arrive as
  `adjustment.updated` once Paddle approves them. Partial refunds and credits are ignored. A
  refunded subscription stays `refunded` (including through the cancellation that usually
  follows, and even while Paddle still reports the subscription active) until a new payment
  completes.
- Won disputes: a `chargeback_reverse` / `chargeback_warning_reverse` adjustment (`status`
  `approved`), or the original `chargeback` / `chargeback_warning` updated to `status`
  `reversed`, lifts the refund only when `refunded_by` is that same action, so it never undoes
  a genuine refund. The state is then re-read from Paddle's subscription; if Paddle cannot be
  reached it is `active` until the stored `current_end` when that is still in the future, else
  `expired`.
- Accounts are found by subscription id; events tagged with a `kyra_ref` are also matched by
  reference, by the pending checkout's email and by Paddle customer. Events for a subscription
  that is neither known nor tagged are acknowledged and skipped, so purchases made outside
  Kyra's checkout cannot create accounts.
- Every event updates the licenses of all devices bound to the account.
- Ordering: events older than the account's `last_event_at` (Paddle `occurred_at`) are
  ignored, and each `event_id` is processed once (Paddle delivers at least once and in no
  guaranteed order). This check runs against the account as stored, before anything else.
  An event for a subscription other than the account's current one switches the account to it
  only when it is a payment (`transaction.completed` with a subscription), or when it is newer
  than `last_event_at` and grants access ending later than the account's current entitlement.
  Anything else for another subscription is acknowledged as superseded, so a late or retried
  event for an old subscription cannot take the account back.
- The purchasing device (from the pending checkout) is bound only the first time a
  subscription is seen, so a device evicted later is not re-added on renewal.
- An account holds at most 3 devices; binding a 4th evicts the oldest and deletes its
  license and reverse index entry.
- `GET /license` answers from KV. When the KV license is missing or expired it asks Paddle at
  most once per device per 10 minutes, which covers a webhook that is late or lost, e.g. while
  the app polls right after checkout: the account's subscription for a bound device, else the
  subscription of the pending checkout transaction this same device started (its `kyra_ref`
  must match). It never does this for a device whose derived reference belongs to an account
  it is not bound to, so the device cap cannot be bypassed, and never for a `refunded` account
  (bound, or the pending checkout's), because Paddle may keep a subscription active after a
  full refund; a new payment restores access through the webhook.
- Cancel, resume, payment method changes and invoices happen in Paddle's customer portal
  (`POST /account/manage`, which creates a customer portal session and returns the
  subscription's `view_subscription` link, else the portal overview). Paddle's own emails
  also link to the portal. The API can cancel subscriptions server-side
  (`POST /subscriptions/{id}/cancel`) if an in-app cancel button is wanted again.

### Known limitations

- A Mac already bound to an account that subscribes again with a different email is credited
  to the account it is bound to: the checkout reuses that account's reference, and webhooks
  match by reference before email. The new email gets no account of its own.

### Security notes

- Webhook: `Paddle-Signature` is `ts=…;h1=…`; the worker computes HMAC-SHA256 of
  `${ts}:${raw body}` with `PADDLE_WEBHOOK_SECRET` and compares it in constant time against
  every `h1` (several appear while a secret rotates). Timestamps more than 5 minutes from
  the worker's clock are rejected (replay). The raw body is verified before it is parsed, and
  nothing is written before the check. A sandbox notification cannot be replayed against a
  production worker: each environment has its own destination secret.
- `/pay` sends a Content-Security-Policy with a per-response nonce for its one inline script
  and allows only the hosts Paddle.js uses (`cdn.paddle.com`, `sandbox-cdn.paddle.com`,
  `buy.paddle.com`, `sandbox-buy.paddle.com`, `*.paddle.com` for its API calls, and
  `public.profitwell.com`, the Retain snippet Paddle.js injects on live accounts), plus
  `frame-ancestors 'none'`, `X-Frame-Options: DENY`, `nosniff` and `no-store`. Interpolated
  values are JSON-encoded with `<`, `>`, `&` and line separators escaped. The page never
  echoes the transaction id; Paddle.js reads it from the URL.
- Restore codes: 6 digits from `crypto.getRandomValues` (rejection-sampled), stored only
  as `SHA-256(salt:email:code)` with a random salt, compared in constant time, 10 minute
  expiry, 5 attempts then the code is destroyed, single use. `restore/start` always
  answers `{ok: true}`, counts its per-email limit for every address (so a 429 reveals
  nothing), and sends the email after responding so timing does not reveal accounts
  either.
- Account and manage are authorised by possession of a bound `device_id`. There is no
  password or session: anyone who learns a bound device id can read the masked account
  and open the customer portal (where Paddle shows the subscription and can cancel it).
  This keeps the app sign-up free. The trade-off only holds if the id is unguessable and
  private, so the app keeps it as a random 256-bit value in its data directory, never shows
  or logs it, and it is never sent to anyone but this worker.
- KV is eventually consistent and not transactional, so rate limits, the attempt counter and
  near-simultaneous webhooks for one account can race slightly. With 3 codes/hour and 5
  attempts per code, brute force stays near 15 guesses/hour against 1,000,000 codes.
- Environments: every license and account carries the `PADDLE_ENV` it was written under and
  is ignored by a worker running in the other one, so test-card purchases cannot unlock
  production even if the KV namespace were shared. Use a separate namespace anyway (see
  Switching to production).
- Only the two configured price ids grant access; a subscription for any other price in the
  same Paddle account (created in the dashboard, or another product) cannot unlock Pawtrol.
- A refund cannot be undone by the license fallback asking Paddle, and a won dispute only
  reverses a refund state that the same dispute caused.
- Upstream errors are logged as status codes and Paddle's fixed error code only; responses
  never echo provider bodies or secrets.

### Paddle references

What this worker relies on, as documented on 2026-10-01:

- Checkout links: a transaction's `checkout.url` is the default payment link (or the
  `checkout.url` passed when creating the transaction, which must be an approved domain) +
  `?_ptxn=<txn id>`; Paddle.js on that page opens the checkout automatically; the default
  payment link must be set before transactions can be created:
  https://developer.paddle.com/build/transactions/default-payment-link,
  https://developer.paddle.com/build/transactions/pass-transaction-checkout
- Create transaction (`POST /transactions`, `items[{price_id, quantity}]`, `customer_id`,
  `custom_data`, `checkout.url`; transaction.write) and custom data being copied from the
  transaction to the subscription and its renewals:
  https://developer.paddle.com/api-reference/transactions/create-transaction,
  https://developer.paddle.com/build/transactions/custom-data
- Customers: emails are unique; `POST /customers` returns 409 `customer_already_exists`
  with "customer email conflicts with customer of id ctm_…"; `GET /customers?email=` exact
  match: https://developer.paddle.com/api-reference/customers/create-customer,
  https://developer.paddle.com/errors/customers/customer_already_exists,
  https://developer.paddle.com/api-reference/customers/list-customers
- Subscription entity (`status` active/canceled/past_due/paused/trialing,
  `current_billing_period` null when paused or canceled, `scheduled_change` {action,
  effective_at}, `management_urls` temporary and not in webhooks):
  https://developer.paddle.com/api-reference/subscriptions/get-subscription,
  https://developer.paddle.com/webhooks/subscriptions/subscription-updated
- Access rules (keep access while past_due, revoke on canceled/paused, keep until a
  scheduled change takes effect): https://developer.paddle.com/build/subscriptions/provision-access-webhooks
- Customer portal sessions (`POST /customers/{id}/portal-sessions` with `subscription_ids`,
  `urls.general.overview`, `urls.subscriptions[].view_subscription`, temporary, not to be
  cached; customer_portal_session.write):
  https://developer.paddle.com/api-reference/customer-portals/create-customer-portal-session,
  https://developer.paddle.com/build/customers/integrate-customer-portal
- Webhooks: payload `{event_id, event_type, occurred_at, notification_id, data}`, at-least-once
  delivery, no ordering guarantee (use `occurred_at`, dedupe on `event_id`), 200 within 5 s,
  retries (sandbox 3 in 15 min, live 60 in 3 days):
  https://developer.paddle.com/webhooks/about/how-webhooks-work,
  https://developer.paddle.com/webhooks/about/respond-to-webhooks
- Signature verification (`Paddle-Signature: ts=…;h1=…`, HMAC-SHA256 of `ts:rawBody`, more
  than one `h1` during secret rotation, SDKs reject after 5 s by default):
  https://developer.paddle.com/webhooks/about/signature-verification
- Adjustments (actions `refund`, `credit`, `chargeback`, `chargeback_warning`,
  `chargeback_reverse`, `chargeback_warning_reverse`, `credit_reverse`; `type` full/partial;
  `pending_approval` → `approved`/`rejected`, and `reversed` on the original when Paddle
  creates its reversal; chargebacks are refunded automatically):
  https://developer.paddle.com/build/transactions/create-transaction-adjustments,
  https://developer.paddle.com/webhooks/adjustments/adjustment-updated
- Paddle.js: load from `https://cdn.paddle.com/paddle/v2/paddle.js`,
  `Paddle.Environment.set("sandbox")` before `Paddle.Initialize({token})`, `checkout.settings`
  defaults (`allowLogout`), `checkout.completed` event:
  https://developer.paddle.com/paddle-js/about/include-paddlejs,
  https://developer.paddle.com/paddle-js/methods/paddle-environment-set,
  https://developer.paddle.com/build/checkout/set-up-checkout-default-settings,
  https://developer.paddle.com/paddle-js/events/checkout-completed
- API hosts (`https://sandbox-api.paddle.com`, `https://api.paddle.com`), Bearer API keys
  (`pdl_sdbx_…` / `pdl_live_…`), `Paddle-Version: 1`, permissions, sandbox differences (no
  website approval, test cards, refunds auto-approved every 10 minutes):
  https://developer.paddle.com/api-reference/about/authentication,
  https://developer.paddle.com/api-reference/about/versioning,
  https://developer.paddle.com/api-reference/about/permissions,
  https://developer.paddle.com/sdks/sandbox
- Customer portal hosts (`customer-portal.paddle.com`, `sandbox-customer-portal.paddle.com`):
  https://developer.paddle.com/changelog/2025/subscription-management-links-customer-portal
- Domain approval (each domain and subdomain that launches a checkout must be approved
  separately): https://www.paddle.com/help/start/account-verification/what-is-domain-approval

Not confirmed by the docs and isolated in `src/paddle.ts` / `src/pay.ts`: the exact set of
hosts Paddle.js needs for a Content-Security-Policy (Paddle publishes none; the list above
is taken from the Paddle.js v2 loader and checked against a sandbox page load), whether a
`workers.dev` subdomain can pass Paddle's live domain review, and the precise behaviour of a
full refund on the subscription itself (the worker ends access on the approved adjustment
and does not depend on Paddle cancelling the subscription).

## Tests

```sh
npm install
npm test            # vitest, in-memory KV, mocked Paddle/Resend/JEV, fake clock
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

### Mock payments (`DEV_MOCK_PADDLE=1`)

With the flag in `.dev.vars` and requests addressed to `127.0.0.1`/`localhost`:

- `/checkout/create` returns `http://127.0.0.1:8787/dev/mock-pay?ref=kyra-…&plan=monthly`
  instead of a Paddle checkout. Opening it sends correctly signed `transaction.completed` and
  `subscription.activated` webhooks through the real webhook handler.
- `/account/manage` returns `http://127.0.0.1:8787/dev/mock-manage?ref=…`, a page with
  links that fire a scheduled cancel, undoing it, a renewal, a failed renewal (`past_due`),
  an immediate cancellation and a full refund.
- `/restore/start` prints the code in the `wrangler dev` output
  (`[dev] restore code for you@example.com: 123456`) instead of emailing it.
- `/license` never calls Paddle.

Mock mode needs both the flag and a loopback request host, so it cannot switch on for a
deployed worker (its requests arrive on the workers.dev or custom hostname). Never add
`DEV_MOCK_PADDLE` to `wrangler.toml` or as a secret.

Without the flag, `wrangler dev` talks to the real sandbox (put a sandbox API key, client
token and destination secret in `.dev.vars`): the checkout link is then
`http://127.0.0.1:8787/pay?_ptxn=…`, which sandbox accepts without domain approval.
Webhooks need a public URL (Paddle suggests a tunnel such as Hookdeck); the license
fallback covers the purchase without them.

A license can also be seeded from the command line (mock checkout + mock webhooks):

```sh
npm run seed-license                                 # reads ~/Library/Application Support/com.kyra.app/device_id.v2
npm run seed-license -- <device_id> --plan yearly    # explicit id / plan
npm run seed-license -- --days 1                     # custom period length
npm run seed-license -- --cancel                     # revoke (sends subscription.canceled)
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

### Paddle dashboard setup

Do everything in the sandbox first (https://sandbox-vendors.paddle.com), then repeat it in the
live account: the two share no data, keys or settings.

1. **Account**: create the sandbox account (https://sandbox-vendors.paddle.com/signup). The
   live account needs Paddle's business verification before it can sell.
2. **Product and prices** (Catalog > Products): create the product "Pawtrol" (tax category
   `saas` or `standard`) and add two recurring prices: **USD 0.99, every 1 month** and
   **USD 9.99, every 1 year**. Copy both price ids (`pri_…`) into `wrangler.toml` as
   `PADDLE_PRICE_ID_MONTHLY` and `PADDLE_PRICE_ID_YEARLY` (plain vars, not secrets; the
   sandbox ids are there already).
3. **Client-side token** (Developer tools > Authentication > Client-side tokens): create one
   (`test_…` in sandbox, `live_…` in live). It is public by design; it only lets Paddle.js
   open checkouts.
4. **Website approval and default payment link** (Checkout > Website approval, Checkout >
   Checkout settings > Default payment link): Paddle only opens checkouts on approved domains,
   and every domain or subdomain needs its own approval (sandbox needs none). Either
   - get the worker's host approved (`kyra-guardian.flashbacks.workers.dev`, or better a custom
     domain such as `pay.kyra.app` routed to this worker) and set the default payment link to
     `https://<that host>/pay`; or
   - host a page that includes Paddle.js on the already approved site (copy `/pay`), set it
     as the default payment link and as `PADDLE_CHECKOUT_URL` in `wrangler.toml`.
   The default payment link is mandatory (Paddle refuses to create transactions without it)
   and is also where Paddle sends customers to update a payment method, so it must be a page
   that loads Paddle.js. Approval reviews the site for product, pricing, terms, refund and
   privacy pages; a bare `workers.dev` host may not pass, so plan for the custom domain or
   the approved site.
   The app opens the checkout link with the opener plugin, which only opens allowlisted
   URLs: any custom checkout host (a custom domain routed to this worker, or the host of
   `PADDLE_CHECKOUT_URL`) must also be added to the `opener:allow-open-url` list in
   `src-tauri/capabilities/default.json`, or the app cannot open the checkout.
5. **Notification destination** (Developer tools > Notifications > New destination): type
   URL (webhook), URL `https://kyra-guardian.flashbacks.workers.dev/webhook/paddle`, API version 1,
   usage type "Platform and simulation" in sandbox ("Platform only" in live), subscribed events:
   `transaction.completed`, `subscription.created`, `subscription.activated`,
   `subscription.updated`, `subscription.canceled`, `subscription.past_due`,
   `subscription.paused`, `subscription.resumed`, `subscription.trialing`,
   `adjustment.created`, `adjustment.updated`.
   Copy the destination's secret key (`pdl_ntfset_…`) for `PADDLE_WEBHOOK_SECRET`.
6. **API key** (Developer tools > Authentication > API keys): create a key (`pdl_sdbx_apikey_…`
   / `pdl_live_apikey_…`) with only these permissions: `customer.write` (create and look up
   customers), `transaction.write` (create and read transactions), `subscription.read` (license
   fallback), `customer_portal_session.write` (Manage button). Give it an expiry you will
   remember to rotate.
7. **Customer portal and emails** (Checkout > Customer portal / branding): set the brand and
   support email so Paddle's receipts and portal look like Kyra.
8. Optional: **Payment recovery / Retain** (live only) controls the dunning that follows a
   failed renewal (`past_due`). Whatever it decides at the end (cancel or pause) ends access.

### Resend

Add and verify the sending domain (the SPF/DKIM DNS records Resend lists), create an API
key with sending access, and choose the from address on that domain, e.g.
`Kyra <hello@kyra.app>`.

### Secrets

```sh
npx wrangler secret put PADDLE_API_KEY          # pdl_sdbx_apikey_… (sandbox) / pdl_live_apikey_…
npx wrangler secret put PADDLE_WEBHOOK_SECRET   # notification destination secret key
npx wrangler secret put PADDLE_CLIENT_TOKEN     # test_… (sandbox) / live_…
npx wrangler secret put RESEND_API_KEY
npx wrangler secret put MAIL_FROM
npx wrangler secret put JEV_API_KEY
```

Plain vars in `wrangler.toml`: `JEV_API_URL`, `PADDLE_ENV` (`sandbox` | `production`),
`PADDLE_PRICE_ID_MONTHLY`, `PADDLE_PRICE_ID_YEARLY`, and optionally `PADDLE_CHECKOUT_URL`.
The price ids belong to one Paddle environment and change when switching to production.

### Ship (sandbox)

```sh
npm test && npm run typecheck
npx wrangler deploy
```

Then run one checkout from the app with a sandbox test card (`4242 4242 4242 4242`, any
name, a future expiry), confirm the notification shows delivered in Developer tools >
Notifications, `GET /license?device_id=…` reports active, `GET /account` shows the plan, and
Manage opens the sandbox customer portal. Repeat for the yearly plan, then cancel from the
portal and check `/account` shows `cancelled`. The webhook simulator (Developer tools >
Simulations) is useful to check delivery and signatures: its events are answered 200 but
skipped, because they do not carry a `kyra_ref` from Kyra's checkout.

### Switching to production

Every step is required. Sandbox and live share nothing in Paddle, and the worker must not
carry sandbox state, keys or ids into production.

1. **Fresh KV.** Create a new KV namespace for production
   (`npx wrangler kv namespace create LICENSES_PROD`) and put its id in the `LICENSES`
   binding in `wrangler.toml`, or purge every key from the current namespace. Sandbox
   licenses and accounts are already ignored by a production worker (`paddle_env`), but
   reverse indexes, pending checkouts and processed event ids would linger.
2. **Live account setup.** Repeat steps 2 to 7 of the dashboard setup in the live account
   (prices, client token, approved domain and default payment link, API key, branding).
3. **Production notification destination.** Create it in the live account (step 5, usage
   type "Platform only", the same event list) pointing at the production worker's
   `/webhook/paddle`.
4. **Replace every Paddle value.** In `wrangler.toml`: `PADDLE_ENV = "production"`, the live
   `PADDLE_PRICE_ID_MONTHLY` and `PADDLE_PRICE_ID_YEARLY`, and `PADDLE_CHECKOUT_URL` if the
   checkout page is not this worker's `/pay`. Secrets, with live values:
   `PADDLE_WEBHOOK_SECRET` (the live destination's key), `PADDLE_API_KEY` (`pdl_live_apikey_…`)
   and `PADDLE_CLIENT_TOKEN` (`live_…`).
5. **App allowlist.** Add any custom checkout host or `PADDLE_CHECKOUT_URL` host to the opener
   allowlist in `src-tauri/capabilities/default.json`.
6. `npx wrangler deploy`, then make one real purchase and refund it to check the flow.

Never ship a release build of the app while the worker it points at is in sandbox: test
cards would unlock Pawtrol for real users.
