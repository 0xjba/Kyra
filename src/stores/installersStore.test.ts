import { beforeEach, describe, expect, it } from "vitest";
import { invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { installer } from "../test/fixtures";
import { useInstallersStore } from "./installersStore";
import { useSettingsStore } from "./settingsStore";

const initial = useInstallersStore.getState();
const initialSettings = useSettingsStore.getState();
const files = [installer("a.dmg", 500), installer("b.pkg", 300), installer("c.iso", 200)];

beforeEach(() => {
  useInstallersStore.setState(initial, true);
  useSettingsStore.setState(initialSettings, true);
});

describe("scan", () => {
  it("lists files with all selected and resets freed", async () => {
    useInstallersStore.setState({ freed: 999 });
    onInvoke("scan_installers", () => files);
    await useInstallersStore.getState().scan();
    const s = useInstallersStore.getState();
    expect(s).toMatchObject({ phase: "list", freed: 0 });
    expect(s.selected).toEqual(new Set(files.map((f) => f.path)));
  });

  it("returns to idle with the error on failure", async () => {
    onInvoke("scan_installers", () => {
      throw new Error("nope");
    });
    await useInstallersStore.getState().scan();
    expect(useInstallersStore.getState().phase).toBe("idle");
    expect(useInstallersStore.getState().error).toContain("nope");
  });
});

describe("selection", () => {
  it("toggles, selects all and deselects all", () => {
    useInstallersStore.setState({ phase: "list", files, selected: new Set() });
    const s = useInstallersStore.getState();
    s.toggleSelect(files[1].path);
    expect(useInstallersStore.getState().selected).toEqual(new Set([files[1].path]));
    s.toggleSelect(files[1].path);
    expect(useInstallersStore.getState().selected.size).toBe(0);
    s.selectAll();
    expect(useInstallersStore.getState().selected.size).toBe(3);
    s.deselectAll();
    expect(useInstallersStore.getState().selected.size).toBe(0);
  });
});

describe("deleteSelected", () => {
  beforeEach(() => {
    useInstallersStore.setState({ phase: "list", files, selected: new Set([files[0].path, files[1].path]) });
    onInvoke("add_bytes_freed", () => 0);
  });

  it("is a no-op with nothing selected", async () => {
    useInstallersStore.setState({ selected: new Set() });
    await useInstallersStore.getState().deleteSelected();
    expect(invokedWith("delete_installers")).toHaveLength(0);
  });

  it("deletes, accumulates freed bytes and keeps survivors unselected on dismiss", async () => {
    onInvoke("delete_installers", () => ({
      items_removed: 1,
      bytes_freed: 500,
      errors: ["b.pkg: in use"],
      deleted_paths: [files[0].path],
    }));
    useInstallersStore.setState({ freed: 100 });
    await useInstallersStore.getState().deleteSelected();

    expect(invokedWith("delete_installers")[0]).toMatchObject({ dryRun: false, permanent: true });
    expect(useInstallersStore.getState()).toMatchObject({ phase: "done", freed: 600 });
    expect(listenerCount("installer-progress")).toBe(0);

    useInstallersStore.getState().dismissDone();
    const s = useInstallersStore.getState();
    expect(s.phase).toBe("list");
    expect(s.files.map((f) => f.name)).toEqual(["b.pkg", "c.iso"]);
    expect(s.selected.size).toBe(0);
    expect(s.result).toBeNull();
  });

  it("stays on an empty list after deleting everything", async () => {
    onInvoke("delete_installers", () => ({
      items_removed: 3,
      bytes_freed: 1000,
      errors: [],
      deleted_paths: files.map((f) => f.path),
    }));
    useInstallersStore.setState({ selected: new Set(files.map((f) => f.path)) });
    await useInstallersStore.getState().deleteSelected();
    useInstallersStore.getState().dismissDone();
    expect(useInstallersStore.getState()).toMatchObject({ phase: "list", files: [], freed: 1000 });
  });

  it("returns to the list with the error on failure", async () => {
    onInvoke("delete_installers", () => {
      throw new Error("locked");
    });
    await useInstallersStore.getState().deleteSelected();
    expect(useInstallersStore.getState().phase).toBe("list");
    expect(useInstallersStore.getState().error).toContain("locked");
    expect(listenerCount("installer-progress")).toBe(0);
  });
});
