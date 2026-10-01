import { describe, expect, it } from "vitest";
import { devicesLine, isValidEmail, planLine, sanitizeCode } from "./pawtrolAccount";
import type { Account } from "../lib/tauri";

const OCT_29_2026 = Date.UTC(2026, 9, 29, 12) / 1000;
const account = (over: Partial<Account> = {}): Account => ({
  email: "j***@gmail.com",
  status: "active",
  current_end: OCT_29_2026,
  cancel_at_period_end: false,
  devices_count: 1,
  ...over,
});

describe("isValidEmail", () => {
  it("accepts ordinary addresses, with surrounding space", () => {
    for (const e of ["me@example.com", " me+kyra@mail.co.uk ", "a.b@c.io"]) expect(isValidEmail(e)).toBe(true);
  });

  it("rejects incomplete or malformed addresses", () => {
    for (const e of ["", "me", "me@", "@x.com", "me@x", "me@x.", "me@.com", "m e@x.com", "me@@x.com", "me@x..com"]) {
      expect(isValidEmail(e), e).toBe(false);
    }
  });
});

describe("sanitizeCode", () => {
  it("keeps the first six digits of whatever was pasted", () => {
    expect(sanitizeCode("123 456")).toBe("123456");
    expect(sanitizeCode("Your code: 987-654-3")).toBe("987654");
    expect(sanitizeCode("12ab")).toBe("12");
  });
});

describe("planLine", () => {
  it("shows price and renewal, or the end date once cancelled", () => {
    expect(planLine(account(), null)).toBe("$0.99/month · renews Oct 29, 2026");
    expect(planLine(account({ cancel_at_period_end: true }), null)).toBe("Ends Oct 29, 2026");
  });

  it("asks for a new card after a failed renewal", () => {
    expect(planLine(account({ status: "billing_issue" }), null)).toBe(
      "Payment failed · update your card under Manage",
    );
  });

  it("falls back to the license expiry without an account", () => {
    expect(planLine(null, OCT_29_2026)).toBe("$0.99/month · renews Oct 29, 2026");
    expect(planLine(null, null)).toBe("$0.99/month");
  });

  it("counts devices against the limit", () => {
    expect(devicesLine(account({ devices_count: 2 }))).toBe("2 of 3 Macs");
  });
});
