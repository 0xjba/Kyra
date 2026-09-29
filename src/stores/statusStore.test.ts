import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emit, invokedWith, listenerCount, listenMock, onInvoke } from "../test/tauri";
import type { DetailedStats } from "../lib/tauri";

const T0 = 10_000_000;

const tick = (overrides: Partial<DetailedStats> = {}): DetailedStats =>
  ({
    cpu_usage: 10,
    net_upload: 1,
    net_download: 2,
    top_processes: [{ name: "kernel_task", cpu: 5, memory: 1 }],
    ...overrides,
  }) as DetailedStats;

async function freshStore() {
  vi.resetModules();
  return (await import("./statusStore")).useStatusStore;
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(T0);
  onInvoke("start_stats_stream", () => undefined);
  onInvoke("stop_stats_stream", () => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("statusStore", () => {
  it("subscribes once even if started twice", async () => {
    const store = await freshStore();
    await Promise.all([store.getState().startStream(), store.getState().startStream()]);
    expect(listenMock).toHaveBeenCalledTimes(1);
    expect(listenerCount("system-stats-tick")).toBe(1);
    expect(invokedWith("start_stats_stream")).toHaveLength(1);
    expect(store.getState().streaming).toBe(true);
  });

  it("drops ticks that arrive closer than 600ms apart", async () => {
    const store = await freshStore();
    await store.getState().startStream();
    emit("system-stats-tick", tick({ cpu_usage: 1 }));
    vi.setSystemTime(T0 + 100);
    emit("system-stats-tick", tick({ cpu_usage: 2 }));
    vi.setSystemTime(T0 + 700);
    emit("system-stats-tick", tick({ cpu_usage: 3 }));
    const s = store.getState();
    expect(s.networkHistory).toHaveLength(2);
    expect(s.stats?.cpu_usage).toBe(3);
  });

  it("keeps the last process list when a tick has none and zeroes non-finite rates", async () => {
    const store = await freshStore();
    await store.getState().startStream();
    emit("system-stats-tick", tick());
    vi.setSystemTime(T0 + 1000);
    emit("system-stats-tick", tick({ top_processes: [], net_upload: NaN, net_download: Infinity }));
    const s = store.getState();
    expect(s.stats?.top_processes).toHaveLength(1);
    expect(s.networkHistory[1]).toEqual({ upload: 0, download: 0 });
  });

  it("caps network history at 60 points", async () => {
    const store = await freshStore();
    await store.getState().startStream();
    for (let i = 0; i < 65; i++) {
      vi.setSystemTime(T0 + i * 1000);
      emit("system-stats-tick", tick({ net_upload: i }));
    }
    const h = store.getState().networkHistory;
    expect(h).toHaveLength(60);
    expect(h[0].upload).toBe(5);
    expect(h[59].upload).toBe(64);
  });

  it("stopStream unsubscribes and stops the backend stream", async () => {
    const store = await freshStore();
    await store.getState().startStream();
    store.getState().stopStream();
    expect(listenerCount("system-stats-tick")).toBe(0);
    expect(invokedWith("stop_stats_stream")).toHaveLength(1);
    expect(store.getState()).toMatchObject({ streaming: false, unlisten: null });
  });

  it("drops a subscription that resolves after stopStream", async () => {
    const store = await freshStore();
    const starting = store.getState().startStream();
    store.getState().stopStream();
    await starting;
    expect(listenerCount("system-stats-tick")).toBe(0);
    expect(invokedWith("start_stats_stream")).toHaveLength(0);
    expect(store.getState().unlisten).toBeNull();
  });

  it("survives the backend refusing to start", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    onInvoke("start_stats_stream", () => {
      throw new Error("busy");
    });
    const store = await freshStore();
    await store.getState().startStream();
    expect(error).toHaveBeenCalled();
    expect(store.getState().streaming).toBe(true);
  });
});
