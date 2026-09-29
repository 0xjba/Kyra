import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { flush, invokedWith, onInvoke } from "../test/tauri";
import { useSettingsStore } from "../stores/settingsStore";
import { useGuardianStore } from "../stores/guardianStore";
import Settings from "./Settings";

vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(async () => {}),
  disable: vi.fn(async () => {}),
  isEnabled: vi.fn(async () => false),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(async () => null) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn(async () => {}) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.4.0") }));

const initial = useSettingsStore.getState();
const initialGuardian = useGuardianStore.getState();

beforeEach(() => {
  useSettingsStore.setState({ loaded: true, settings: { ...initial.settings, onboarding_completed: true } });
  onInvoke("save_settings", () => undefined);
});

afterEach(() => {
  cleanup();
  useSettingsStore.setState(initial, true);
  useGuardianStore.setState(initialGuardian, true);
});

const rowDesc = (name: string) =>
  screen.getByText(name, { selector: ".st-row-name" }).parentElement!.querySelector(".st-row-desc")?.textContent;

function renderSettings() {
  return render(
    <MemoryRouter initialEntries={["/settings"]}>
      <Routes>
        <Route path="/settings" element={<Settings />} />
        <Route path="/guardian" element={<div>pawtrol page</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Pawtrol settings", () => {
  beforeEach(() => {
    useGuardianStore.setState({ checkLicense: async () => {} });
  });

  it("has no Pawtrol rules row without a license, and keeps the low-disk stepper", () => {
    renderSettings();
    expect(screen.queryByText("Pawtrol rules")).toBeNull();
    expect(rowDesc("Low disk space alert")).toBe("Warn when free space drops below");
    const row = screen.getByText("Low disk space alert", { selector: ".st-row-name" }).closest(".st-row") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Increase" }));
    expect(invokedWith("save_settings").pop()).toMatchObject({ settings: { low_disk_threshold_gb: 15 } });
  });

  it("moves the Pawtrol controls to the Pawtrol page", () => {
    useGuardianStore.setState({ license: { active: true, expires: null } });
    renderSettings();
    expect(screen.queryByRole("switch", { name: "Run automatically" })).toBeNull();
    expect(screen.queryByRole("switch", { name: "Auto-clean safe items" })).toBeNull();
    expect(rowDesc("Pawtrol rules")).toBe("Schedule, low-space alerts and what it cleans");
    expect(rowDesc("Low disk space alert")).toBe("Managed by Pawtrol");
  });

  it.each(["Open", "View rules"])("%s goes to the Rules tab", async (label) => {
    useGuardianStore.setState({ license: { active: true, expires: null }, pawtrolTab: "overview" });
    renderSettings();
    fireEvent.click(screen.getByRole("button", { name: label }));
    expect(await screen.findByText("pawtrol page")).toBeTruthy();
    expect(useGuardianStore.getState().pawtrolTab).toBe("rules");
  });
});

const OCT_29_2026 = Date.UTC(2026, 9, 29, 12) / 1000;
const ACCOUNT = {
  email: "j***@gmail.com",
  status: "active",
  current_end: OCT_29_2026,
  cancel_at_period_end: false,
  devices_count: 2,
};


describe("Pawtrol account", () => {
  beforeEach(() => {
    useGuardianStore.setState({ checkLicense: async () => {}, license: { active: true, expires: OCT_29_2026 } });
    onInvoke("guardian_account", () => ACCOUNT);
  });

  it("shows account, plan and devices instead of a manage link", async () => {
    renderSettings();
    await act(flush);
    expect(rowDesc("Account")).toBe("j***@gmail.com");
    expect(rowDesc("Plan")).toBe("$0.99/month · renews Oct 29, 2026");
    expect(rowDesc("Devices")).toBe("2 of 3 Macs");
    expect(screen.queryByText("Manage subscription")).toBeNull();
  });

  it("cancels only after confirmation, then shows the end date", async () => {
    onInvoke("guardian_cancel_subscription", () => ({ ...ACCOUNT, cancel_at_period_end: true }));
    renderSettings();
    await act(flush);

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    const dialog = screen.getByRole("alertdialog", { name: "Cancel Pawtrol?" });
    expect(within(dialog).getByText("It stays on until Oct 29, 2026. You can resubscribe any time.")).toBeTruthy();
    expect(invokedWith("guardian_cancel_subscription")).toHaveLength(0);

    fireEvent.click(within(dialog).getByRole("button", { name: "Keep Pawtrol" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(invokedWith("guardian_cancel_subscription")).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel subscription" }));
    await act(flush);

    expect(invokedWith("guardian_cancel_subscription")).toHaveLength(1);
    expect(rowDesc("Plan")).toBe("Ends Oct 29, 2026");
    expect((screen.getByRole("button", { name: "Cancelled" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows a cancel failure inline", async () => {
    onInvoke("guardian_cancel_subscription", () => {
      throw "Pawtrol's server had a hiccup. Try again in a minute.";
    });
    renderSettings();
    await act(flush);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel subscription" }));
    await act(flush);
    expect(rowDesc("Cancel subscription")).toBe("Pawtrol's server had a hiccup. Try again in a minute.");
    expect(rowDesc("Plan")).toBe("$0.99/month · renews Oct 29, 2026");
  });
});

describe("Pawtrol entry points in Settings", () => {
  beforeEach(() => {
    useGuardianStore.setState({ checkLicense: async () => {} });
  });

  it("Subscribe opens the email sheet and Restore opens the restore sheet", () => {
    renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    expect(screen.getByRole("dialog", { name: "Subscribe to Pawtrol" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(screen.getByRole("dialog", { name: "Restore Pawtrol" })).toBeTruthy();
  });
});

describe("Settings", () => {
  it("Replay resets onboarding_completed and returns home", async () => {
    render(
      <MemoryRouter initialEntries={["/settings"]}>
        <Routes>
          <Route path="/" element={<div>home page</div>} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Replay" }));

    expect(useSettingsStore.getState().settings.onboarding_completed).toBe(false);
    expect(invokedWith("save_settings").pop()).toMatchObject({ settings: { onboarding_completed: false } });
    expect(await screen.findByText("home page")).toBeTruthy();
  });
});
