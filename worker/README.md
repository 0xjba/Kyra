# Pawtrol worker

Cloudflare Worker behind Pawtrol ($0.99/month or $9.99/year): licenses, the scoring proxy,
RevenueCat subscriptions with Paddle Billing as the payment provider (a RevenueCat Web
Purchase Link hosts plan selection on `pay.rev.cat` and embeds a Paddle checkout; Paddle is
merchant of record and runs the subscription), email-code restore and account management.
Everything is stored in the `LICENSES` KV namespace.

## Endpoints

| Method | Path | Body / query | Response |
| --- | --- | --- | --- |
| GET | `/license` | `?device_id=` | `{active, expires}` (`expires` null when unknown) |
| POST | `/jev/score` | `{device_id, questions[]}` | upstream JSON; needs a license, 10/hour/device |
| POST | `/webhook/revenuecat` | RevenueCat event, `Authorization` header | `{ok, action \| message}` |
| POST | `/checkout/create` | `{device_id, email}` | `{short_url, app_user_id}` |
| POST | `/restore/start` | `{email}` | always `{ok: true}` |
| POST | `/restore/verify` | `{email, code, device_id}` | `{active, expires, app_user_id}` |
| GET | `/account` | `?device_id=` | `{email (masked), status, current_end, cancel_at_period_end, devices_count, management_url, plan}` or 404; `plan` is `monthly`, `yearly` or null |
| POST | `/account/manage` | `{device_id}` | `{url}`: https link to the Paddle customer portal (see "Subscription management") |
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
| `license:{device_id}` | `{active, expires, app_user_id, environment}` | max(35 d, period + 5 d) |
| `account:{email}` | `{email, app_user_id, status, current_end, grace_end, cancel_at_period_end, management_url, plan, environment, devices: [{device_id, bound_at}], last_event_at}` | none |
| `device:{device_id}` | email (reverse index) | none |
| `rcuser:{app_user_id}` | email (reverse index) | none |
| `pending:{app_user_id}` | `{device_id, email}` from checkout | 7 d |
| `restore:{email}` | `{hash, salt, expires, attempts}` | 10 min |
| `rcsync:{device_id}` | last RevenueCat license refresh | 10 min |
| `ratelimit:{device_id}`, `rl:checkout:{ip}`, `rl:manage:{ip}`, `rl:restore-ip:{ip}`, `rl:restore-email:{email}` | fixed-window counters | 1 h |

Emails are trimmed and lower-cased before use as keys.

`environment` is `SANDBOX` or `PRODUCTION`, from the webhook event's `environment` or the
REST subscription's `environment`. A record without it counts as `SANDBOX`.

### Subscription rules

- Handled webhook events: `INITIAL_PURCHASE`, `RENEWAL`, `UNCANCELLATION`,
  `PRODUCT_CHANGE` (access until `expiration_at_ms`); `CANCELLATION` (access until the
  period ends, except a refund, which ends it now, and `cancel_reason` `BILLING_ERROR`,
  the retry that accompanies `BILLING_ISSUE`); `BILLING_ISSUE` (access until
  `grace_period_expiration_at_ms`, or the period end without a grace period); `EXPIRATION`
  (access ends); `TRANSFER` (the account follows the purchases from `transferred_from` to
  `transferred_to[0]`). Everything else is acknowledged with 200 and ignored.
- Handling does not depend on `store`: `PADDLE` events (this setup) and `RC_BILLING` events
  take the same path. Webhook events are not filtered by `entitlement_ids` (the project
  has a single entitlement); the REST fallback and Manage only consider subscriptions that
  grant `REVENUECAT_ENTITLEMENT`.
- Refunds: RevenueCat sends `CANCELLATION` with `cancel_reason` `CUSTOMER_SUPPORT` for
  refunded web subscriptions. `REFUND` and a negative `price` ("negative for refunds") are
  treated as refunds too. A refund for an earlier period sends no event, and a refund that
  leaves the Paddle subscription running is followed by the next `RENEWAL`, which restores
  access.
- `plan`: `yearly` or `monthly` when the product id (`new_product_id` on a product change
  that has one) is listed in `REVENUECAT_PRODUCT_YEARLY` / `REVENUECAT_PRODUCT_MONTHLY`
  (comma-separated; imported Paddle prices keep Paddle's `pri_…` id as the RevenueCat
  product id). Otherwise the period length decides (`expiration_at_ms - purchased_at_ms`):
  more than 60 days is `yearly`, anything shorter `monthly`. Events that reveal neither
  keep the stored plan. The RevenueCat fallback derives it the same way from
  `product_id` and `current_period_starts_at`/`current_period_ends_at`.
- Environments: with `REVENUECAT_ALLOW_SANDBOX=1` both are accepted; without it only
  `PRODUCTION`. Lifecycle events from another environment (or without one) are
  acknowledged and ignored, and stored licenses and accounts from another environment are
  treated as absent everywhere (license checks, scoring, account, manage, restore,
  checkout). Sandbox purchases made while testing therefore stop counting the moment the
  flag is removed, and a leaked sandbox link cannot unlock Pawtrol with Paddle test cards.
  Set the flag only while testing.
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
  RevenueCat (the `REVENUECAT_ENTITLEMENT` entitlement on the customer's subscriptions) at
  most once per device per 10 minutes, which covers a webhook that is late or lost, e.g.
  while the app polls right after checkout. It never does this for a device that is not bound to an
  account that already exists, so the device cap cannot be bypassed.
- `POST /checkout/create` answers 409 `already_active` when this Mac's account is entitled
  ("Pawtrol is already active on this Mac") or when the email's account is entitled ("This
  email already has an active Pawtrol subscription"); the app sends that customer to
  Restore instead of starting a second subscription.

### Subscription management

Paddle runs the subscription, so cancel, resume and card changes happen in Paddle's
customer portal (`customer-portal.paddle.com`, sandbox `sandbox-customer-portal.paddle.com`);
RevenueCat's own customer portal is not used with Paddle. `POST /account/manage`:

1. Lists the customer's subscriptions (`GET /projects/{pid}/customers/{cid}/subscriptions`)
   and picks the one that grants the `REVENUECAT_ENTITLEMENT` entitlement.
2. Asks `GET /projects/{pid}/subscriptions/{id}/authenticated_management_url`. The API
   reference says its `management_url` is, for Paddle subscriptions, "a short-lived
   authenticated Paddle Customer Portal URL when the API key has the Customer portal session
   (Write) permission and Paddle returns the required management URLs; otherwise a
   non-authenticated URL (customer signs in via email) or `null`".
3. If that call fails or returns no https URL, uses the subscription's own `management_url`
   (RevenueCat embeds Paddle's authenticated session token in it when the Paddle key has
   Customer portal sessions: Write). The endpoint's summary still only names the "Web
   Billing customer portal", hence the fallback.
4. Answers `{url}` (https only), 409 `no_active_subscription` without one, 502 when the
   subscription list itself fails.

The server-side cancel and refund endpoints of the REST API are Web Billing only and do not
apply to Paddle subscriptions.

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
- `checkout/create` does reveal whether an email has an active subscription (409
  `already_active`), which is the price of refusing duplicate subscriptions. It is limited
  to 10 requests per hour per IP.
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

- Paddle Billing integration (API key permissions, `pay.rev.cat` website approval, product
  import, Web Purchase Links with Paddle as the payment provider, sandbox testing, "Paddle
  acts as the billing engine and merchant of record", RevenueCat's customer portal is not
  used): https://www.revenuecat.com/docs/web/integrations/paddle
- Web Purchase Links: `https://pay.rev.cat/<token>/<url-encoded app_user_id>`, `?email=`
  presets a non-editable email; a production and a sandbox URL per link; the app_user_id
  is required (404 without it); Paddle Billing is a supported billing engine:
  https://www.revenuecat.com/docs/web/web-billing/web-purchase-links
- Engine comparison (RevenueCat Billing, Stripe Billing and Paddle Billing all work with
  Web Purchase Links): https://www.revenuecat.com/docs/web/overview
- Webhooks (Authorization header, 200 required, retries 5x, out-of-order delivery) and
  event fields (`event_timestamp_ms`, `purchased_at_ms`, `expiration_at_ms` and "subtract
  `purchased_at_ms` from `expiration_at_ms` to get the period duration",
  `grace_period_expiration_at_ms`, `cancel_reason`, `price` negative for refunds,
  `transferred_from/to`, `subscriber_attributes`, `store` including `PADDLE`,
  `environment` `SANDBOX`/`PRODUCTION`; refunds send `CANCELLATION`, only for the latest
  period): https://www.revenuecat.com/docs/integrations/webhooks,
  https://www.revenuecat.com/docs/integrations/webhooks/event-types-and-fields
- REST API v2 (Bearer v2 secret key; `GET /projects/{id}/customers/{id}/subscriptions`,
  `GET /projects/{id}/subscriptions/{id}/authenticated_management_url` with
  `customer_information:subscriptions:read` and its Paddle behaviour quoted above;
  subscription `store` including `paddle`, `environment` `sandbox`/`production`,
  `management_url`; 480 requests/min for customer information):
  https://www.revenuecat.com/docs/api-v2,
  https://www.revenuecat.com/docs/api-v2/subscription,
  https://www.revenuecat.com/docs/api-v2/customer/resources

Not confirmed by the docs and isolated in `src/revenuecat.ts` / `src/webhook.ts`: whether
RevenueCat copies the checkout email into the `$email` attribute (the pending checkout
covers it); which `cancel_reason` a Paddle refund carries (the cancellation-reasons table
names only RevenueCat Billing and Stripe Billing for `CUSTOMER_SUPPORT`, so `REFUND` and a
negative `price` are accepted as well); and how long the Paddle portal URLs stay valid
(both are fetched fresh on every Manage click).

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
  `INITIAL_PURCHASE` webhook (store `PADDLE`, environment `PRODUCTION`, monthly plan;
  append `&days=365` for a yearly one) through the real webhook handler.
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

### RevenueCat + Paddle setup

Do the Paddle steps in the Paddle **sandbox** account (`https://sandbox-vendors.paddle.com/`)
first, then repeat them in the live account (`https://login.paddle.com/`) before launch.
Paddle sandbox and live are separate accounts, each with its own key and prices.

**Paddle**

- [ ] Paddle Billing is active (needed for the product import).
- [ ] Catalog > Products: a Pawtrol product with two recurring prices, USD 0.99 / 1 month and
      USD 9.99 / 1 year. Each Paddle price becomes one RevenueCat product.
- [ ] Developer Tools > Authentication > **+ New API key**, set to **not expire** (the
      default is to expire), with the permissions RevenueCat lists:
      - Read: Addresses, Adjustments, Businesses, Client-side tokens, Customers, Discounts,
        Notification settings, Notifications, Payment methods, Prices, Products,
        Subscriptions, Transactions.
      - Write: Client-side tokens, Customer portal sessions, Notification settings,
        Transactions.
      Customer portal sessions (Write) is what lets RevenueCat put authenticated Paddle
      portal links in `management_url`; without it Manage cannot open the portal signed in.
      Keep the key window open until the key is saved in RevenueCat.
- [ ] Checkout > Website approval: add `pay.rev.cat`. Not required in sandbox; required
      live, and Paddle reviews it, which can take a while.
- [ ] Checkout > Checkout settings: if no default payment link is set, enter
      `https://pay.rev.cat`.
- [ ] Checkout recovery: turn off abandoned cart emails (their link cannot reopen a checkout
      RevenueCat started).

**RevenueCat**

- [ ] Create a project.
- [ ] Web > add a **Paddle** config: set the Paddle API key as its secret, choose
      **Automatic purchase tracking** (and autogenerated user IDs), then **Connect to Paddle**.
      Optionally Webhook Configuration > **Apply in Paddle** for faster updates.
- [ ] Product catalog > Products > the Paddle provider > **Import** both Paddle prices. Put
      their RevenueCat product ids (the `pri_…` price ids) in `REVENUECAT_PRODUCT_MONTHLY`
      and `REVENUECAT_PRODUCT_YEARLY` in `wrangler.toml`.
- [ ] Entitlements: create the Pawtrol entitlement (this project uses **`kyra_pawtrol`**) and
      attach both products. The entitlement identifier must match `REVENUECAT_ENTITLEMENT`
      (a plain var in `wrangler.toml`; `pawtrol` when unset).
- [ ] Offerings: create **`default`** with two packages, monthly ($0.99 product) and yearly
      ($9.99 product). The plan is chosen on RevenueCat's hosted page.
- [ ] Funnels > Purchase Links: create a Web Purchase Link for the `default` offering with
      **Paddle** as the payment provider; set the success behaviour to the default success
      page (or a page that says "return to Kyra") and a terms link. Share URL > **Copy
      sandbox URL** for testing (the production URL for launch), without the app user id:
      `https://pay.rev.cat/<token>`.
- [ ] Integrations > Webhooks: URL
      `https://kyra-guardian.flashbacks.workers.dev/webhook/revenuecat`, Authorization header
      value: a long random string (the same value goes into REVENUECAT_WEBHOOK_AUTH), all events; production, plus sandbox while
      testing.
- [ ] Project settings > API keys: create a **v2 secret key** with
      `customer_information:subscriptions:read` (reads customers and subscriptions; it is
      also the only permission `authenticated_management_url` requires; customer reads use
      the same Customer Information domain). Note the project id (`proj…`).

Paddle sends receipts and subscription emails. Sandbox purchases use Paddle's test card
`4242 4242 4242 4242`, any future expiry, CVC `100`. Paddle's webhook simulator is ignored by
RevenueCat; test renewals with real sandbox purchases (sandbox periods are full length).

### Resend

Add and verify the sending domain (the SPF/DKIM DNS records Resend lists), create an API
key with sending access, and choose the from address on that domain, e.g.
`Kyra <hello@kyra.app>`.

### Secrets

Copy each value, then pipe it in so it never lands in shell history:

```sh
pbpaste | npx wrangler secret put REVENUECAT_SECRET_API_KEY      # sk_… (v2)
pbpaste | npx wrangler secret put REVENUECAT_PROJECT_ID          # proj…
pbpaste | npx wrangler secret put REVENUECAT_WEBHOOK_AUTH        # exactly the value pasted into RevenueCat's Authorization field
pbpaste | npx wrangler secret put REVENUECAT_WEB_PURCHASE_LINK   # https://pay.rev.cat/<token> (sandbox URL while testing)
pbpaste | npx wrangler secret put REVENUECAT_ALLOW_SANDBOX       # 1, testing only (see below)
pbpaste | npx wrangler secret put RESEND_API_KEY
pbpaste | npx wrangler secret put MAIL_FROM
pbpaste | npx wrangler secret put JEV_API_KEY
```

Before launch, put the production purchase link and remove the sandbox flag
(`npx wrangler secret delete REVENUECAT_ALLOW_SANDBOX`). Every sandbox license and account
then stops counting.

`JEV_API_URL`, `REVENUECAT_ENTITLEMENT`, `REVENUECAT_PRODUCT_MONTHLY` and
`REVENUECAT_PRODUCT_YEARLY` are plain vars in `wrangler.toml`. Paddle sandbox and live
accounts have separate prices with different ids: add the live ids (comma-separated) when
going live. Ids that are not listed fall back to the period length.

### Ship

```sh
npm test && npm run typecheck
npx wrangler deploy
```

Then, with `REVENUECAT_ALLOW_SANDBOX=1` and the sandbox purchase link as
`REVENUECAT_WEB_PURCHASE_LINK`, run one checkout with the Paddle test card, confirm the
webhook shows 200 in RevenueCat (store `PADDLE`, environment `SANDBOX`),
`GET /license?device_id=…` reports active, `GET /account` shows the plan, and Manage opens
`sandbox-customer-portal.paddle.com`. Switch both back to production values before release.
