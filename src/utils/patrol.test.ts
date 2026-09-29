import { describe, expect, it } from "vitest";
import { agoLabel, dataLossCopy, nextPatrolLabel, patrolStatusLine, runSummary } from "./patrol";
import { patrolRun, patrolStatus } from "../test/fixtures";

const NOW = new Date(2026, 8, 29, 14, 0).getTime();
const at = (h: number, m = 0, dayOffset = 0) => new Date(2026, 8, 29 + dayOffset, h, m).getTime() / 1000;
const S = NOW / 1000;

describe("patrol labels", () => {
  it("formats how long ago", () => {
    expect(agoLabel(S - 10, NOW)).toBe("just now");
    expect(agoLabel(S - 300, NOW)).toBe("5m ago");
    expect(agoLabel(S - 3 * 3600, NOW)).toBe("3h ago");
    expect(agoLabel(S - 30 * 3600, NOW)).toBe("yesterday");
    expect(agoLabel(S - 5 * 86400, NOW)).toBe("5d ago");
  });

  it("formats the next patrol", () => {
    expect(nextPatrolLabel(S + 30, NOW)).toBe("next any minute");
    expect(nextPatrolLabel(S + 20 * 60, NOW)).toBe("next in 20 min");
    expect(nextPatrolLabel(at(16), NOW)).toBe("next around 4 PM");
    expect(nextPatrolLabel(at(21), NOW)).toBe("next around tonight");
    expect(nextPatrolLabel(at(9, 0, 1), NOW)).toBe("next tomorrow morning");
    expect(nextPatrolLabel(at(9, 0, 3), NOW)).toBe("next Friday");
  });

  it("builds the status line for each state", () => {
    const base = { last_patrol_at: S - 3 * 3600, next_patrol_at: at(21) };
    expect(patrolStatusLine(patrolStatus(base), NOW)).toBe("Last run 3h ago · next around tonight");
    expect(patrolStatusLine(patrolStatus({ ...base, running: true }), NOW)).toBe("Checking now…");
    expect(patrolStatusLine(patrolStatus({ ...base, enabled: false }), NOW)).toBe("Paused · last run 3h ago");
    expect(patrolStatusLine(patrolStatus({ last_patrol_at: null, next_patrol_at: null }), NOW)).toBe("Hasn't run yet");
  });

  it("summarises a run", () => {
    expect(runSummary(patrolRun())).toEqual({ title: "Found nothing", detail: "Everything was already tidy" });
    expect(runSummary(patrolRun({ review_count: 1 })).detail).toBe("Found 1 for you to review");
    expect(runSummary(patrolRun({ error: "boom" }))).toEqual({ title: "Didn't finish", detail: "boom" });
    const cleaned = ["a", "b", "c", "d"].map((name) => ({ name, size: 1 }));
    expect(runSummary(patrolRun({ cleaned, freed: 2048 })).detail).toBe("Cleaned a, b and 2 more");
  });

  it("writes the data-loss copy for trash and permanent deletes", () => {
    expect(dataLossCopy(["Deletes models"], true)).toBe("Deletes models. It goes to the Trash, so you can still restore it.");
    expect(dataLossCopy(["Deletes models."], false)).toBe("Deletes models. This can't be undone.");
  });
});
