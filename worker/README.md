# Pawtrol worker

Cloudflare Worker behind Pawtrol ($0.99/month): licenses, the scoring proxy, Razorpay
subscriptions, email-code restore and account management. Everything is stored in the
`LICENSES` KV namespace.

## Endpoints

| Method | Path | Body / query | Response |
| --- | --- | --- | --- |
| GET | `/license` | `?device_id=` | `{active, expires}` (`expires` null when unknown) |
| POST | `/jev/score` | `{device_id, questions[]}` | upstream JSON; needs a license, 10/hour/device |
| POST | `/webhook/razorpay` | Razorpay event, `X-Razorpay-Signature` | `{ok, action \| message}` |
| POST | `/checkout/create` | `{device_id, email}` | `{short_url, subscription_id}` |
| POST | `/restore/start` | `{email}` | always `{ok: true}` |
| POST | `/restore/verify` | `{email, code, device_id}` | `{active, expires}` |
| GET | `/account` | `?device_id=` | `{email (masked), status, current_end, cancel_at_period_end, devices_count}` or 404 |
| POST | `/subscription/cancel` | `{device_id}` | same shape as `/account` |

New endpoints return errors as `{error, code}`. `code` is one of `invalid_json`,
`invalid_email`, `invalid_device_id`, `invalid_code`, `code_expired`,
`too_many_attempts`, `rate_limited`, `subscription_inactive`, `not_found`,
`no_active_subscription`, `payment_provider_error`, `internal_error`. The original three
endpoints keep their `{error}` shape. `device_id` must be 64 lowercase hex characters on
the new endpoints.

### KV layout

| Key | Value | TTL |
| --- | --- | --- |
| `license:{device_id}` | `{active, expires, subscription_id}` | max(35 d, period + 5 d) |
| `account:{email}` | `{email, subscription_id, status, current_end, cancel_at_period_end, devices: [{device_id, bound_at}], last_event_at}` | none |
| `device:{device_id}` | email (reverse index) | none |
| `pending:{subscription_id}` | `{device_id, email}` from checkout | 7 d |
| `restore:{email}` | `{hash, salt, expires, attempts}` | 10 min |
| `ratelimit:{device_id}`, `rl:checkout:{ip}`, `rl:restore-ip:{ip}`, `rl:restore-email:{email}` | fixed-window counters | 1 h |

Emails are trimmed and lower-cased before use as keys.

### Subscription rules

- License expiry = `current_end` while `active`; `current_end + 3 days` while `pending`
  (a failed renewal being retried); `current_end` after a cancel-at-cycle-end; none after
  an immediate cancel, `halted`, `completed` or `paused`. `authenticated` records the
  account and binds the device but grants nothing until `activated`.
- Every event updates the licenses of all devices bound to the account.
- Events older than the account's `last_event_at` (Razorpay's top-level `created_at`) are
  ignored, so retries and out-of-order deliveries are harmless. Late non-live events from
  an older subscription never override a newer active one.
- The purchasing device (from `notes`) is bound only the first time a subscription is
  seen, so a device evicted later is not re-added on renewal.
- An account holds at most 3 devices; binding a 4th evicts the oldest and deletes its
  license and reverse index entry.

### Security notes

- Webhook: HMAC-SHA256 over the raw body, verified with `crypto.subtle.verify`
  (constant time). Nothing is written before verification.
- Restore codes: 6 digits from `crypto.getRandomValues` (rejection-sampled), stored only
  as `SHA-256(salt:email:code)` with a random salt, compared in constant time, 10 minute
  expiry, 5 attempts then the code is destroyed, single use. `restore/start` always
  answers `{ok: true}`, counts its per-email limit for every address (so a 429 reveals
  nothing), and sends the email after responding so timing does not reveal accounts
  either.
- Account and cancel are authorised by possession of a bound `device_id`. There is no
  password or session: anyone who learns a bound device id can read the masked account
  and cancel the renewal (never refund or re-bind). This keeps the app sign-up free.
  The trade-off only holds if the id is unguessable and private, so the app should keep
  it as a random 256-bit value in its data directory and never show or log it. Moving to
  a server-issued secret per device would remove the dependency.
- KV is eventually consistent and not transactional, so rate limits and the attempt
  counter can overshoot slightly under concurrent requests. With 3 codes/hour and 5
  attempts per code, brute force stays near 15 guesses/hour against 1,000,000 codes.
  A Durable Object would make them exact if ever needed.
- Upstream errors are logged as status codes only; responses never echo provider bodies
  or secrets.

## Tests

```sh
npm install
npm test            # vitest, in-memory KV, mocked Razorpay/Resend/JEV, fake clock
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

### Mock payments (`DEV_MOCK_RAZORPAY=1`)

With the flag in `.dev.vars` and requests addressed to `127.0.0.1`/`localhost`:

- `/checkout/create` returns `http://127.0.0.1:8787/dev/mock-pay?subscription_id=sub_mock_…`
  instead of calling Razorpay. Opening it sends signed `subscription.activated` and
  `subscription.charged` webhooks through the real webhook handler.
- `/restore/start` prints the code in the `wrangler dev` output
  (`[dev] restore code for you@example.com: 123456`) instead of emailing it.
- `/subscription/cancel` skips the Razorpay call.

Mock mode needs both the flag and a loopback request host, so it cannot switch on for a
deployed worker (its requests arrive on the workers.dev or custom hostname). Never add
`DEV_MOCK_RAZORPAY` to `wrangler.toml` or as a secret.

Without the checkout flow, a license can also be seeded directly:

```sh
npm run seed-license                          # reads ~/Library/Application Support/com.kyra.app/device_id.v2
npm run seed-license -- <device_id> --days 1  # explicit id / length
npm run seed-license -- --cancel              # revoke (sends subscription.cancelled)
```

Mock scores are deterministic: >= 1 GB scores 90, >= 100 MB scores 55, else 20.

Rust client against the same local worker (after `npm run seed-license -- e2e-dev`):

```sh
cd src-tauri && cargo test guardian -- --ignored
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

### Razorpay

1. Settings > International payments: enable it. USD plans and non-INR cards require
   this, and it needs a Razorpay review.
2. Subscriptions > Plans > Create plan: monthly, interval 1, amount 0.99 USD. Copy the
   `plan_…` id.
3. Settings > API keys: generate a live key id and secret (test keys for staging).
4. Settings > Webhooks > Add: URL `https://<worker-host>/webhook/razorpay`, a strong
   random secret, and the events `subscription.authenticated`, `subscription.activated`,
   `subscription.charged`, `subscription.pending`, `subscription.halted`,
   `subscription.cancelled`, `subscription.completed`, `subscription.paused`,
   `subscription.resumed`.

### Resend

Add and verify the sending domain (the SPF/DKIM DNS records Resend lists), create an API
key with sending access, and choose the from address on that domain, e.g.
`Kyra <hello@kyra.app>`.

### Secrets

```sh
npx wrangler secret put RAZORPAY_KEY_ID
npx wrangler secret put RAZORPAY_KEY_SECRET
npx wrangler secret put RAZORPAY_WEBHOOK_SECRET
npx wrangler secret put RAZORPAY_PLAN_ID
npx wrangler secret put RESEND_API_KEY
npx wrangler secret put MAIL_FROM
npx wrangler secret put JEV_API_KEY
```

`JEV_API_URL` is a plain var in `wrangler.toml`.

### Ship

```sh
npm test && npm run typecheck
npx wrangler deploy
```

Then run one real checkout with Razorpay test keys, confirm the webhook shows 200 in the
Razorpay dashboard and `GET /license?device_id=…` reports active.
