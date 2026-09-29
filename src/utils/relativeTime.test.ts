import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ageText, ageTier, daysSinceUsed, downloadedAgo, lastUsedLabel } from "./relativeTime";

const NOW = new Date(2026, 5, 15, 12, 0, 0);
const secsDaysAgo = (days: number) => Math.floor(NOW.getTime() / 1000) - days * 86400;
const localNoonDaysAgo = (days: number) =>
  new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate() - days, 12).getTime() / 1000;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("downloadedAgo", () => {
  it("handles a missing timestamp", () => {
    expect(downloadedAgo(0)).toBe("Download date unknown");
  });

  it.each([
    [0.5, "Downloaded today"],
    [1, "Downloaded yesterday"],
    [5, "Downloaded 5 days ago"],
    [13, "Downloaded 13 days ago"],
    [14, "Downloaded 2 weeks ago"],
    [45, "Downloaded 6 weeks ago"],
    [60, "Downloaded 2 months ago"],
    [364, "Downloaded 12 months ago"],
    [365, "Downloaded 1 year ago"],
    [800, "Downloaded 2 years ago"],
  ])("%s days ago -> %s", (days, label) => {
    expect(downloadedAgo(secsDaysAgo(days))).toBe(label);
  });
});

describe("daysSinceUsed", () => {
  it("counts calendar days, not 24h periods", () => {
    const lateYesterday = new Date(2026, 5, 14, 23, 30).getTime() / 1000;
    expect(daysSinceUsed(lateYesterday)).toBe(1);
    const earlyToday = new Date(2026, 5, 15, 0, 5).getTime() / 1000;
    expect(daysSinceUsed(earlyToday)).toBe(0);
  });

  it("clamps future timestamps to 0", () => {
    expect(daysSinceUsed(localNoonDaysAgo(-3))).toBe(0);
  });
});

describe("lastUsedLabel", () => {
  it.each([
    [0, "Used today"],
    [1, "Used yesterday"],
    [3, "Used 3 days ago"],
    [7, "Used last week"],
    [21, "Used 3 weeks ago"],
    [45, "Used last month"],
    [75, "Used 2 months ago"],
    [90, "Used 3 months ago"],
    [91, "Last used 3 months ago"],
    [400, "Last used 1 year ago"],
    [800, "Last used 2 years ago"],
  ])("%s days -> %s", (days, label) => {
    expect(lastUsedLabel(localNoonDaysAgo(days))).toBe(label);
  });
});

describe("prune age helpers", () => {
  it("tiers projects by idle days", () => {
    expect(ageTier(null)).toBe("");
    expect(ageTier(29)).toBe("");
    expect(ageTier(30)).toBe(" idle");
    expect(ageTier(89)).toBe(" idle");
    expect(ageTier(90)).toBe(" stale");
  });

  it("describes activity", () => {
    expect(ageText(null)).toBe("Activity unknown");
    expect(ageText(0)).toBe("Active today");
    expect(ageText(5)).toBe("Active 5d ago");
    expect(ageText(30)).toBe("30 days idle");
  });
});
