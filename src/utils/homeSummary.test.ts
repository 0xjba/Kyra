import { describe, expect, it } from "vitest";
import type { SystemStats } from "../lib/tauri";
import type { Summaries } from "../stores/summaryStore";
import { daysSinceOptimized, homeSpeech, tileValue } from "./homeSummary";

const GB = 1024 * 1024 * 1024;
const at = 1_700_000_000_000;

const stats: SystemStats = {
  cpu_usage: 10,
  memory_total: 16 * GB,
  memory_used: 8 * GB,
  memory_percent: 50,
  disk_total: 500 * GB,
  disk_free: 200 * GB,
};

describe("homeSpeech", () => {
  it("invites a first scan when nothing has been scanned", () => {
    const s = homeSpeech({ cleans: 0 }, stats);
    expect(s.cta).toBe("Start scanning");
    expect(s.to).toBe("/clean");
  });

  it("points at the module with the most reclaimable space", () => {
    const summaries: Summaries = {
      cleans: 0,
      clean: { bytes: 1 * GB, count: 3, at },
      prune: { bytes: 3 * GB, count: 4, projects: 2, at },
      installers: { bytes: 2 * GB, count: 1, at },
    };
    const s = homeSpeech(summaries, stats);
    expect(s.b).toBe("6.0 GB");
    expect(s.cta).toBe("Review it all");
    expect(s.to).toBe("/prune");
  });

  it("reports tidy with disk usage once scans found nothing", () => {
    const summaries: Summaries = { cleans: 1, clean: { bytes: 0, count: 0, at } };
    expect(homeSpeech(summaries, stats)).toMatchObject({ a: "All tidy. ", b: "300 GB", cta: "Scan anyway" });
    expect(homeSpeech(summaries, null).b).toBe("Your Mac");
  });
});

describe("tileValue", () => {
  it("distinguishes never scanned, tidy, and found", () => {
    expect(tileValue(undefined)).toBeUndefined();
    expect(tileValue(0)).toBe("Tidy");
    expect(tileValue(2 * GB)).toBe("2.0 GB");
  });
});

describe("daysSinceOptimized", () => {
  it("returns null before the first run and whole days after", () => {
    expect(daysSinceOptimized(undefined)).toBeNull();
    expect(daysSinceOptimized(at, at + 1000)).toBe(0);
    expect(daysSinceOptimized(at, at + 3.5 * 86_400_000)).toBe(3);
  });
});
