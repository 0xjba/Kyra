import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { flush, invokedWith, onInvoke } from "../test/tauri";
import { patrolRules, patrolRun, patrolStatus, reviewItem, toWireStatus } from "../test/fixtures";
import { useGuardianStore } from "../stores/guardianStore";
import { useSettingsStore } from "../stores/settingsStore";
import type { PatrolRules, PatrolRun, PatrolStatus } from "../lib/tauri";
import Guardian from "./Guardian";

const initial = useGuardianStore.getState();
const initialSettings = useSettingsStore.getState();
const GB = 1024 * 1024 * 1024;
const DOCKER_LOSS = "Deletes all Docker images, containers and volumes";

const seeded = () =>
  patrolStatus({
    freed_total: 12.4 * GB,
    freed_last: 1.2 * GB,
    pending_review: [
      reviewItem("docker", { name: "Docker Data", size: 5 * GB, score: 55, user_data: true, data_loss: DOCKER_LOSS }),
      reviewItem("python", { name: "Python", details: "pip cache", size: 0.5 * GB, score: 62 }),
      reviewItem("npm", { name: "npm cache", size: 2 * GB, score: 92, safe: true }),
    ],
    history: [
      patrolRun({
        started_at: Math.floor(Date.now() / 1000) - 7200,
        finished_at: Math.floor(Date.now() / 1000) - 7100,
        trigger: "low_disk",
        cleaned: [],
        error: "Disk was busy",
      }),
      patrolRun({
        freed: 1.2 * GB,
        cleaned: [
          { name: "npm cache", size: 0.7 * GB },
          { name: "Xcode DerivedData", size: 0.5 * GB },
        ],
        review_count: 2,
      }),
    ],
  });

async function renderDashboard(status: PatrolStatus = seeded(), useTrash = true) {
  useSettingsStore.setState({ settings: { ...initialSettings.settings, use_trash: useTrash } });
  onInvoke("guardian_patrol_status", () => toWireStatus(status));
  render(<Guardian />);
  await act(flush);
}

const row = (name: string) => screen.getByText(name).closest(".guardian-review-row") as HTMLElement;

beforeEach(() => {
  onInvoke("guardian_get_device_id", () => "dev-1");
  onInvoke("guardian_check_license", () => ({ active: true, expires: null }));
  onInvoke("get_device_name", () => "MacBook Pro");
  onInvoke("guardian_review_clean", () => ({ categories_cleaned: 1, bytes_freed: 1, errors: [] }));
  onInvoke("guardian_review_dismiss", () => undefined);
  onInvoke("guardian_set_rules", () => undefined);
});

afterEach(() => {
  cleanup();
  useGuardianStore.getState().unsubscribePatrol();
  useGuardianStore.setState(initial, true);
  useSettingsStore.setState(initialSettings, true);
});

describe("Pawtrol dashboard", () => {
  it("shows status, the review queue and activity from the patrol status", async () => {
    await renderDashboard();

    expect(screen.getByText("Pawtrol is on duty")).toBeTruthy();
    expect(screen.getByText(/^Last run 3h ago · next /)).toBeTruthy();
    expect(screen.getByText("12.4 GB")).toBeTruthy();
    expect(screen.getByText("1.2 GB last run")).toBeTruthy();

    expect(screen.getByText(DOCKER_LOSS)).toBeTruthy();
    const names = [...document.querySelectorAll(".guardian-review-row .guardian-row-name")].map((n) => n.textContent);
    expect(names).toEqual(["npm cache", "Python", "Docker Data"]);
    expect(screen.getByRole("button", { name: "Clean all safe (2.0 GB)" })).toBeTruthy();

    const activity = screen.getByText("Activity").closest("section") as HTMLElement;
    const titles = [...activity.querySelectorAll(".guardian-activity-title")].map((n) => n.textContent);
    expect(titles).toEqual(["Didn't finish", "Freed 1.2 GB"]);
    expect(within(activity).getByText("Disk was busy")).toBeTruthy();
    expect(within(activity).getByText("Cleaned npm cache, Xcode DerivedData · 2 for you to review")).toBeTruthy();
  });

  it("shows the walking state while a patrol runs", async () => {
    await renderDashboard(patrolStatus({ running: true }));
    expect(screen.getAllByText("Checking now…").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: /Patrol now|Run now/ })).toBeNull();
  });

  it("offers to resume a paused patrol", async () => {
    await renderDashboard(patrolStatus({ enabled: false, rules: patrolRules({ enabled: false, frequency: "weekly" }) }));
    expect(screen.getByText("Pawtrol is paused")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Resume Pawtrol" }));
    await act(flush);
    expect(invokedWith("guardian_set_rules")).toEqual([{ rules: patrolRules({ enabled: true, frequency: "weekly" }) }]);
    expect(screen.getByText("Pawtrol is on duty")).toBeTruthy();
  });

  it("cleans regenerable items without a confirmation", async () => {
    await renderDashboard();
    fireEvent.click(within(row("Python")).getByRole("button", { name: "Clean" }));
    await act(flush);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(invokedWith("guardian_review_clean")).toEqual([{ ids: ["python"] }]);
  });

  it("clean all safe cleans only the safe group", async () => {
    await renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: "Clean all safe (2.0 GB)" }));
    await act(flush);
    expect(invokedWith("guardian_review_clean")).toEqual([{ ids: ["npm"] }]);
  });

  it("requires confirmation before cleaning user data", async () => {
    await renderDashboard();
    fireEvent.click(within(row("Docker Data")).getByRole("button", { name: "Clean" }));

    const dialog = screen.getByRole("alertdialog", { name: "Clean Docker Data?" });
    expect(dialog.textContent).toContain(`${DOCKER_LOSS}. It goes to the Trash, so you can still restore it.`);
    expect(invokedWith("guardian_review_clean")).toHaveLength(0);

    fireEvent.click(within(dialog).getByRole("button", { name: "Move to Trash" }));
    await act(flush);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(invokedWith("guardian_review_clean")).toEqual([{ ids: ["docker"] }]);
  });

  it("cancelling the confirmation cleans nothing", async () => {
    await renderDashboard();
    fireEvent.click(within(row("Docker Data")).getByRole("button", { name: "Clean" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(invokedWith("guardian_review_clean")).toHaveLength(0);
  });

  it("says it can't be undone when Move to Trash is off", async () => {
    await renderDashboard(seeded(), false);
    fireEvent.click(within(row("Docker Data")).getByRole("button", { name: "Clean" }));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.textContent).toContain(`${DOCKER_LOSS}. This can't be undone.`);
    expect(dialog.textContent).not.toContain("Trash");
    expect(within(dialog).getByRole("button", { name: "Delete" })).toBeTruthy();
  });

  it("a selection that includes user data also asks first", async () => {
    await renderDashboard();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select Python" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Select Docker Data" }));
    fireEvent.click(screen.getByRole("button", { name: "Clean 5.5 GB" }));
    expect(screen.getByRole("alertdialog", { name: "Clean 2 items (5.5 GB)?" })).toBeTruthy();
    expect(invokedWith("guardian_review_clean")).toHaveLength(0);
  });

  it("not now dismisses the item", async () => {
    await renderDashboard();
    fireEvent.click(within(row("Docker Data")).getByRole("button", { name: "Not now" }));
    await act(flush);
    expect(invokedWith("guardian_review_dismiss")).toEqual([{ ids: ["docker"] }]);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});

describe("Pawtrol activity", () => {
  it("labels each kind of run, including critical space", async () => {
    const run = (trigger: PatrolRun["trigger"], minsAgo: number) =>
      patrolRun({ trigger, started_at: Math.floor(Date.now() / 1000) - minsAgo * 60, finished_at: Math.floor(Date.now() / 1000) - minsAgo * 60 });
    await renderDashboard(patrolStatus({ history: [run("critical", 1), run("low_disk", 2), run("schedule", 3), run("manual", 4)] }));
    const activity = screen.getByText("Activity").closest("section") as HTMLElement;
    const labels = [...activity.querySelectorAll(".guardian-activity-icon")].map((n) => n.getAttribute("title"));
    expect(labels).toEqual(["Space critical", "Low space", "Scheduled run", "You asked"]);
    expect(activity.querySelector(".guardian-activity-critical")).toBeTruthy();
  });

  it("says when a low-space-only Pawtrol will first run", async () => {
    await renderDashboard(
      patrolStatus({ last_patrol_at: null, next_patrol_at: null, rules: patrolRules({ frequency: "low_disk_only" }) }),
    );
    expect(screen.getByText("Runs when space is low")).toBeTruthy();
    expect(screen.getByText("Pawtrol hasn't run yet. It runs when space is low.")).toBeTruthy();
  });
});

describe("Pawtrol rules", () => {
  async function renderRules(rules: Partial<PatrolRules> = {}) {
    await renderDashboard(patrolStatus({ enabled: rules.enabled ?? true, rules: patrolRules(rules) }));
    fireEvent.click(screen.getByRole("tab", { name: "Rules" }));
  }

  const group = (name: string) => screen.getByRole("radiogroup", { name });
  const choice = (groupName: string, option: string) => within(group(groupName)).getByRole("radio", { name: option });
  const lastRules = () => (invokedWith("guardian_set_rules").pop() as { rules: PatrolRules }).rules;
  const button = (name: string) => screen.getByRole("button", { name }) as HTMLButtonElement;

  it("switches between Overview and Rules and remembers the choice", async () => {
    await renderDashboard();
    expect(screen.getByRole("tab", { name: "Overview" }).getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByRole("tab", { name: "Rules" }));
    expect(screen.queryByText("Pawtrol is on duty")).toBeNull();
    expect(screen.getByText("What Pawtrol does")).toBeTruthy();
    expect(localStorage.getItem("kyra.pawtrol.tab")).toBe("rules");
    fireEvent.click(screen.getByRole("tab", { name: "Overview" }));
    expect(screen.getByText("Pawtrol is on duty")).toBeTruthy();
  });

  it("renders the saved rules", async () => {
    await renderRules({ frequency: "weekly", low_gb: 25, critical_gb: 4, safe_action: "ask", review_action: "quiet", data_action: "ignore" });
    expect((screen.getByRole("switch", { name: "Pawtrol" }) as HTMLElement).getAttribute("aria-checked")).toBe("true");
    expect(choice("How often", "Weekly").getAttribute("aria-checked")).toBe("true");
    expect(choice("Safe caches", "Ask me first").getAttribute("aria-checked")).toBe("true");
    expect(choice("Worth a look", "List quietly").getAttribute("aria-checked")).toBe("true");
    expect(choice("Your data", "Ignore").getAttribute("aria-checked")).toBe("true");
    expect(screen.getByText("Run right away below 25 GB")).toBeTruthy();
    expect(screen.getByText("Below 4 GB, clean safe caches immediately and alert you")).toBeTruthy();
    expect(screen.getByText("Runs when your Mac is idle and plugged in.")).toBeTruthy();
  });

  it.each([
    ["How often", "Every 6 hours", { frequency: "6h" }],
    ["How often", "Only when space is low", { frequency: "low_disk_only" }],
    ["Safe caches", "Ask me first", { safe_action: "ask" }],
    ["Worth a look", "List quietly", { review_action: "quiet" }],
    ["Your data", "List quietly", { data_action: "quiet" }],
    ["Your data", "Ignore", { data_action: "ignore" }],
  ] as const)("%s → %s saves the rule", async (groupName, option, patch) => {
    await renderRules();
    fireEvent.click(choice(groupName, option));
    await act(flush);
    expect(invokedWith("guardian_set_rules")).toEqual([{ rules: patrolRules(patch) }]);
    expect(choice(groupName, option).getAttribute("aria-checked")).toBe("true");
  });

  it("choosing the current option saves nothing", async () => {
    await renderRules();
    fireEvent.click(choice("How often", "Daily"));
    await act(flush);
    expect(invokedWith("guardian_set_rules")).toHaveLength(0);
  });

  it("the switch pauses Pawtrol", async () => {
    await renderRules();
    fireEvent.click(screen.getByRole("switch", { name: "Pawtrol" }));
    await act(flush);
    expect(lastRules()).toEqual(patrolRules({ enabled: false }));
    expect(screen.getByText("Paused. Nothing runs until you turn it back on.")).toBeTruthy();
  });

  it("steps the low-space level by 5 GB within 5–100", async () => {
    await renderRules({ low_gb: 20, critical_gb: 5 });
    fireEvent.click(button("Raise low-space level"));
    await act(flush);
    expect(lastRules()).toMatchObject({ low_gb: 25, critical_gb: 5 });
    expect(screen.getByText("Run right away below 25 GB")).toBeTruthy();

    cleanup();
    await renderRules({ low_gb: 100 });
    expect(button("Raise low-space level").disabled).toBe(true);
    cleanup();
    await renderRules({ low_gb: 5, critical_gb: 4 });
    expect(button("Lower low-space level").disabled).toBe(true);
  });

  it("keeps the critical level below the low-space level", async () => {
    await renderRules({ low_gb: 10, critical_gb: 8 });
    fireEvent.click(button("Raise critical level"));
    await act(flush);
    expect(lastRules()).toMatchObject({ critical_gb: 9 });
    expect(button("Raise critical level").disabled).toBe(true);

    fireEvent.click(button("Lower low-space level"));
    await act(flush);
    expect(lastRules()).toMatchObject({ low_gb: 5, critical_gb: 4 });
    expect(screen.getByText("Below 4 GB, clean safe caches immediately and alert you")).toBeTruthy();
  });

  it("won't lower the critical level below 1 GB", async () => {
    await renderRules({ critical_gb: 1 });
    expect(button("Lower critical level").disabled).toBe(true);
  });

  it("disables everything but the switch while paused", async () => {
    await renderRules({ enabled: false });
    expect(screen.getByText("Paused. Nothing runs until you turn it back on.")).toBeTruthy();
    for (const name of ["How often", "Safe caches", "Worth a look", "Your data"]) {
      for (const radio of within(group(name)).getAllByRole("radio")) expect((radio as HTMLButtonElement).disabled).toBe(true);
    }
    for (const name of ["Raise low-space level", "Lower low-space level", "Raise critical level", "Lower critical level"]) {
      expect(button(name).disabled).toBe(true);
    }
    const toggle = screen.getByRole("switch", { name: "Pawtrol" }) as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    fireEvent.click(toggle);
    await act(flush);
    expect(lastRules()).toEqual(patrolRules({ enabled: true }));
    expect((choice("Your data", "Ignore") as HTMLButtonElement).disabled).toBe(false);
  });

  it("shows a save failure and puts the old rule back", async () => {
    onInvoke("guardian_set_rules", () => {
      throw "Couldn't save settings";
    });
    await renderRules();
    fireEvent.click(choice("Safe caches", "Ask me first"));
    await act(flush);
    expect(screen.getByText("Couldn't save settings")).toBeTruthy();
    expect(choice("Safe caches", "Clean automatically").getAttribute("aria-checked")).toBe("true");
  });
});

describe("Pawtrol paywall", () => {
  it("sells the autonomy when unlicensed", async () => {
    onInvoke("guardian_check_license", () => ({ active: false, expires: null }));
    render(<Guardian />);
    await act(flush);
    expect(screen.getByText(/Pawtrol looks after your MacBook Pro every day/)).toBeTruthy();
    expect(screen.getByText("Auto-cleans safe caches on its own")).toBeTruthy();
    expect(invokedWith("guardian_patrol_status")).toHaveLength(0);
  });

  it("Subscribe and Restore open the shared sheets", async () => {
    onInvoke("guardian_check_license", () => ({ active: false, expires: null }));
    render(<Guardian />);
    await act(flush);

    fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    expect(screen.getByRole("dialog", { name: "Subscribe to Pawtrol" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    fireEvent.click(screen.getByRole("button", { name: "Already subscribed? Restore" }));
    expect(screen.getByRole("dialog", { name: "Restore Pawtrol" })).toBeTruthy();
  });

  it("keeps the restore success step up after the license flips to the dashboard", async () => {
    onInvoke("guardian_check_license", () => ({ active: false, expires: null }));
    onInvoke("guardian_restore_start", () => undefined);
    onInvoke("guardian_restore_verify", () => ({ active: true, expires: null }));
    onInvoke("guardian_patrol_status", () => toWireStatus(patrolStatus()));
    render(<Guardian />);
    await act(flush);

    fireEvent.click(screen.getByRole("button", { name: "Already subscribed? Restore" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Email" }), { target: { value: "me@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await act(flush);
    fireEvent.change(screen.getByRole("textbox", { name: "6-digit code" }), { target: { value: "123456" } });
    await act(flush);

    expect(screen.getByText("Pawtrol is back on this Mac")).toBeTruthy();
    expect(screen.getByText("Pawtrol is on duty")).toBeTruthy();
  });
});
