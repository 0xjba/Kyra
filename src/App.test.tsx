import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { enable } from "@tauri-apps/plugin-autostart";
import { flush, invokedWith, onInvoke } from "./test/tauri";
import { useSettingsStore } from "./stores/settingsStore";
import { useGuardianStore } from "./stores/guardianStore";
import { PawtrolPresence } from "./App";

vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(async () => {}),
  disable: vi.fn(async () => {}),
  isEnabled: vi.fn(async () => false),
}));

const initialSettings = useSettingsStore.getState();
const initialGuardian = useGuardianStore.getState();

function setLicensed(active: boolean) {
  useGuardianStore.setState({ license: { active, expires: null } });
}

function renderPresence() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <PawtrolPresence />
      <Routes>
        <Route path="/" element={<div>home page</div>} />
        <Route path="/guardian" element={<div>pawtrol page</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.mocked(enable).mockClear();
  onInvoke("set_tray_visible", () => undefined);
  onInvoke("save_settings", () => undefined);
  useSettingsStore.setState({ loaded: true, settings: { ...initialSettings.settings, onboarding_completed: true } });
});

afterEach(() => {
  cleanup();
  useSettingsStore.setState(initialSettings, true);
  useGuardianStore.setState(initialGuardian, true);
});

describe("PawtrolPresence", () => {
  it("shows the tray only while Pawtrol is licensed", async () => {
    renderPresence();
    await flush();
    expect(invokedWith("set_tray_visible")).toEqual([{ visible: false }]);

    act(() => setLicensed(true));
    await flush();
    expect(invokedWith("set_tray_visible").pop()).toEqual({ visible: true });
  });

  it("turns on launch at login once when Pawtrol activates", async () => {
    setLicensed(true);
    renderPresence();
    await act(flush);

    expect(enable).toHaveBeenCalledTimes(1);
    expect(useSettingsStore.getState().settings).toMatchObject({ launch_at_login: true, pawtrol_login_prompted: true });
  });

  it("leaves launch at login alone after the one-time prompt", async () => {
    useSettingsStore.setState({
      settings: { ...useSettingsStore.getState().settings, pawtrol_login_prompted: true },
    });
    setLicensed(true);
    renderPresence();
    await act(flush);

    expect(enable).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().settings.launch_at_login).toBe(false);
  });
});
