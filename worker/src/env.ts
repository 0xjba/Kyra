export interface Env {
  LICENSES: KVNamespace;
  JEV_API_KEY: string;
  JEV_API_URL: string;
  /** "sandbox" (default) or "production": picks the Paddle API host and the Paddle.js environment. */
  PADDLE_ENV?: string;
  PADDLE_API_KEY: string;
  /** Secret key of the Paddle notification destination, used to verify `Paddle-Signature`. */
  PADDLE_WEBHOOK_SECRET: string;
  PADDLE_PRICE_ID_MONTHLY: string;
  PADDLE_PRICE_ID_YEARLY: string;
  /** Client-side token for Paddle.js on the /pay page (public by design). */
  PADDLE_CLIENT_TOKEN: string;
  /** Optional page on an approved domain used as the transaction checkout URL; defaults to this worker's /pay. */
  PADDLE_CHECKOUT_URL?: string;
  RESEND_API_KEY: string;
  MAIL_FROM: string;
  DEV_MOCK_PADDLE?: string;
}

export interface Ctx {
  waitUntil(promise: Promise<unknown>): void;
}
