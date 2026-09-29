export interface Env {
  LICENSES: KVNamespace;
  JEV_API_KEY: string;
  JEV_API_URL: string;
  RAZORPAY_WEBHOOK_SECRET: string;
  RAZORPAY_KEY_ID: string;
  RAZORPAY_KEY_SECRET: string;
  RAZORPAY_PLAN_ID: string;
  RESEND_API_KEY: string;
  MAIL_FROM: string;
  DEV_MOCK_RAZORPAY?: string;
}

export interface Ctx {
  waitUntil(promise: Promise<unknown>): void;
}
