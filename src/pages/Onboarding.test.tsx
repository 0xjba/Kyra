import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { flush, onInvoke } from "../test/tauri";
import { useGuardianStore } from "../stores/guardianStore";
import Onboarding from "./Onboarding";

vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(async () => {}),
  disable: vi.fn(async () => {}),
}));

const initial = useGuardianStore.getState();

beforeEach(() => {
  onInvoke("get_device_name", () => "MacBook Pro");
  onInvoke("check_full_disk_access", () => false);
  useGuardianStore.setState({ checkLicense: async () => {} });
});

afterEach(() => {
  cleanup();
  useGuardianStore.setState(initial, true);
});

async function toProStep() {
  render(
    <MemoryRouter>
      <Onboarding />
    </MemoryRouter>,
  );
  await act(flush);
  for (const label of ["Get started", "Next", "Skip for now", "Next"]) {
    fireEvent.click(screen.getByRole("button", { name: label }));
  }
}

describe("Onboarding Pro card", () => {
  it("Subscribe opens the shared email sheet", async () => {
    await toProStep();
    fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    expect(screen.getByRole("dialog", { name: "Subscribe to Pawtrol" })).toBeTruthy();
  });

  it("Restore opens the shared restore sheet", async () => {
    await toProStep();
    fireEvent.click(screen.getByRole("button", { name: "Already subscribed? Restore" }));
    expect(screen.getByRole("dialog", { name: "Restore Pawtrol" })).toBeTruthy();
  });
});
