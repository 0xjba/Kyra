export interface Env {
  LICENSES: KVNamespace;
  JEV_API_KEY: string;
  JEV_API_URL: string;
  REVENUECAT_SECRET_API_KEY: string;
  REVENUECAT_PROJECT_ID: string;
  REVENUECAT_WEBHOOK_AUTH: string;
  REVENUECAT_WEB_PURCHASE_LINK: string;
  // Entitlement identifier that unlocks Pawtrol; "pawtrol" when unset.
  REVENUECAT_ENTITLEMENT?: string;
  // Product ids (comma-separated) of the monthly and yearly products, for `plan`.
  REVENUECAT_PRODUCT_MONTHLY?: string;
  REVENUECAT_PRODUCT_YEARLY?: string;
  // "1" accepts sandbox purchases (testing only); a leaked sandbox link would otherwise grant free access.
  REVENUECAT_ALLOW_SANDBOX?: string;
  RESEND_API_KEY: string;
  MAIL_FROM: string;
  DEV_MOCK_REVENUECAT?: string;
}

export interface Ctx {
  waitUntil(promise: Promise<unknown>): void;
}
