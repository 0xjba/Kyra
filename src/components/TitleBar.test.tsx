import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { flush, onInvoke } from "../test/tauri";
import { useGuardianStore } from "../stores/guardianStore";
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
