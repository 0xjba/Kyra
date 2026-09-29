import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { flush, onInvoke } from "../test/tauri";
import { useGuardianStore } from "../stores/guardianStore";
import { patrolRules, patrolStatus } from "../test/fixtures";
import TitleBar from "./TitleBar";

const initial = useGuardianStore.getState();

beforeEach(() => {
  onInvoke("get_traffic_lights", () => null);
  useGuardianStore.setState({ checkLicense: async () => {} });
});

afterEach(() => {
  cleanup();
  useGuardianStore.setState(initial, true);
});

async function openPopover() {
  render(
    <MemoryRouter initialEntries={["/clean"]}>
      <TitleBar />
    </MemoryRouter>,
  );
  await act(flush);
  fireEvent.click(screen.getByRole("button", { name: /Pawtrol/ }));
}

describe("Pawtrol popover (free)", () => {
  it("Subscribe goes straight to the email sheet and closes the popover", async () => {
    await openPopover();
    fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    expect(screen.getByRole("dialog", { name: "Subscribe to Pawtrol" })).toBeTruthy();
    expect(screen.queryByText("Already subscribed? Restore")).toBeNull();
  });

  it("Already subscribed? Restore opens the restore sheet", async () => {
    await openPopover();
    fireEvent.click(screen.getByRole("button", { name: "Already subscribed? Restore" }));
    expect(screen.getByRole("dialog", { name: "Restore Pawtrol" })).toBeTruthy();
  });
});

describe("Pawtrol popover (pro)", () => {
  const openWith = async (status: ReturnType<typeof patrolStatus>) => {
    useGuardianStore.setState({ license: { active: true, expires: null }, patrolStatus: status, loadPatrolStatus: async () => {} });
    await openPopover();
  };

  it("says a low-space-only Pawtrol runs when space is low", async () => {
    await openWith(patrolStatus({ last_patrol_at: null, next_patrol_at: null, rules: patrolRules({ frequency: "low_disk_only" }) }));
    expect(screen.getByText("Runs when space is low")).toBeTruthy();
  });

  it("shows a paused Pawtrol as paused", async () => {
    await openWith(patrolStatus({ enabled: false, last_patrol_at: null, next_patrol_at: null }));
    expect(screen.getByText("Paused until you resume it")).toBeTruthy();
    expect(screen.getByText("OFF")).toBeTruthy();
  });
});
