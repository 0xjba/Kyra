import { afterEach, beforeEach, vi } from "vitest";
import { resetTauri } from "./tauri";

vi.mock("@tauri-apps/api/core", async () => {
  const m = await import("./tauri");
  return { invoke: m.invokeMock };
});

vi.mock("@tauri-apps/api/event", async () => {
  const m = await import("./tauri");
  return { listen: m.listenMock };
});

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

beforeEach(() => {
  resetTauri();
  localStorage.clear();
});

afterEach(() => {
  vi.useRealTimers();
});
