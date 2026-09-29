import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { flush, invokedWith, onInvoke } from "../test/tauri";
import { patrolRun, patrolStatus, reviewItem, toWireStatus } from "../test/fixtures";
import { useGuardianStore } from "../stores/guardianStore";
import { useSettingsStore } from "../stores/settingsStore";
import type { PatrolStatus } from "../lib/tauri";
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
  onInvoke("guardian_set_patrol", () => undefined);
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
    await renderDashboard(patrolStatus({ enabled: false, auto_clean: true }));
    expect(screen.getByText("Pawtrol is paused")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Resume Pawtrol" }));
    await act(flush);
    expect(invokedWith("guardian_set_patrol")).toEqual([{ enabled: true, autoClean: true }]);
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
