import { beforeEach, describe, expect, it } from "vitest";
import { flush, invokedWith, listenerCount, onInvoke } from "../test/tauri";
import { app } from "../test/fixtures";
import { useUninstallStore } from "./uninstallStore";

const initial = useUninstallStore.getState();
const apps = [app("Slack", 300), app("Zoom", 200), app("Figma", 100)];
const slackFiles = [
  { path: "/Users/me/Library/Caches/com.example.slack", category: "Caches", size: 10, is_dir: true },
  { path: "/Users/me/Library/Preferences/com.example.slack.plist", category: "Preferences", size: 1, is_dir: false },
];

beforeEach(() => {
  useUninstallStore.setState(initial, true);
  onInvoke("add_bytes_freed", () => 0);
});

describe("scanApps", () => {
  it("lists apps", async () => {
    onInvoke("scan_installed_apps", () => apps);
    await useUninstallStore.getState().scanApps();
    expect(useUninstallStore.getState()).toMatchObject({ phase: "list", apps });
  });

  it("returns to idle with the error on failure", async () => {
    onInvoke("scan_installed_apps", () => {
      throw new Error("denied");
    });
    await useUninstallStore.getState().scanApps();
    expect(useUninstallStore.getState().phase).toBe("idle");
    expect(useUninstallStore.getState().error).toContain("denied");
  });
});

describe("app selection", () => {
  it("loads associated files and selects them all", async () => {
    onInvoke("get_associated_files", () => slackFiles);
    await useUninstallStore.getState().selectApp(apps[0]);
    expect(invokedWith("get_associated_files")).toEqual([
      { bundleId: apps[0].bundle_id, appName: "Slack", appPath: apps[0].path },
    ]);
    const s = useUninstallStore.getState();
    expect(s.loadingFiles).toBe(false);
    expect(s.selectedFilePaths).toEqual(new Set(slackFiles.map((f) => f.path)));
  });

  it("stops loading and reports the error when files can't be read", async () => {
    onInvoke("get_associated_files", () => {
      throw new Error("sandbox");
    });
    await useUninstallStore.getState().selectApp(apps[0]);
    expect(useUninstallStore.getState()).toMatchObject({ loadingFiles: false, selectedApp: apps[0] });
    expect(useUninstallStore.getState().error).toContain("sandbox");
  });

  it("toggles files and clears everything on deselectApp", () => {
    useUninstallStore.setState({ selectedApp: apps[0], associatedFiles: slackFiles, selectedFilePaths: new Set() });
    const s = useUninstallStore.getState();
    s.toggleFile(slackFiles[0].path);
    expect(useUninstallStore.getState().selectedFilePaths).toEqual(new Set([slackFiles[0].path]));
    s.selectAllFiles();
    expect(useUninstallStore.getState().selectedFilePaths.size).toBe(2);
    s.deselectAllFiles();
    expect(useUninstallStore.getState().selectedFilePaths.size).toBe(0);
    s.deselectApp();
    expect(useUninstallStore.getState()).toMatchObject({ selectedApp: null, associatedFiles: [] });
  });
});

describe("uninstall", () => {
  beforeEach(() => {
    useUninstallStore.setState({
      phase: "list",
      apps,
      selectedApp: apps[0],
      associatedFiles: slackFiles,
      selectedFilePaths: new Set([slackFiles[0].path]),
    });
  });

  it("is a no-op without a selected app", async () => {
    useUninstallStore.setState({ selectedApp: null });
    await useUninstallStore.getState().uninstall(false);
    expect(invokedWith("execute_uninstall")).toHaveLength(0);
  });

  it("drops the app once its bundle is deleted and refreshes the list", async () => {
    const refreshed = apps.slice(1);
    onInvoke("execute_uninstall", () => ({
      items_removed: 2,
      bytes_freed: 310,
      errors: [],
      deleted_paths: [apps[0].path, slackFiles[0].path],
    }));
    onInvoke("scan_installed_apps", () => refreshed);

    await useUninstallStore.getState().uninstall(true);

    expect(invokedWith("execute_uninstall")[0]).toMatchObject({
      appPath: apps[0].path,
      filePaths: [slackFiles[0].path],
      bundleId: apps[0].bundle_id,
      dryRun: false,
      permanent: true,
    });
    const s = useUninstallStore.getState();
    expect(s.phase).toBe("done");
    expect(s.selectedApp).toBeNull();
    expect(s.apps.map((a) => a.name)).toEqual(["Zoom", "Figma"]);
    expect(listenerCount("uninstall-progress")).toBe(0);
    await flush();
    expect(useUninstallStore.getState().apps).toEqual(refreshed);
  });

  it("keeps the app selected when its bundle survived", async () => {
    onInvoke("execute_uninstall", () => ({
      items_removed: 1,
      bytes_freed: 10,
      errors: ["Slack.app: permission denied"],
      deleted_paths: [slackFiles[0].path],
    }));
    onInvoke("scan_installed_apps", () => apps);
    await useUninstallStore.getState().uninstall(false);
    const s = useUninstallStore.getState();
    expect(s.phase).toBe("done");
    expect(s.selectedApp).toEqual(apps[0]);
    expect(s.apps).toHaveLength(3);
  });

  it("returns to the list with the error on failure", async () => {
    onInvoke("execute_uninstall", () => {
      throw new Error("running");
    });
    await useUninstallStore.getState().uninstall(false);
    expect(useUninstallStore.getState().phase).toBe("list");
    expect(useUninstallStore.getState().error).toContain("running");
    expect(listenerCount("uninstall-progress")).toBe(0);
  });
});

describe("bulkUninstall", () => {
  it("aggregates results and only removes apps that were actually deleted", async () => {
    useUninstallStore.setState({ phase: "list", apps });
    onInvoke("get_associated_files", (args) => {
      if (args?.appName === "Zoom") throw new Error("no files");
      return [];
    });
    onInvoke("execute_uninstall", (args) => {
      if (args?.appPath === apps[2].path) throw new Error("busy");
      return { items_removed: 1, bytes_freed: 100, errors: [], deleted_paths: [args?.appPath as string] };
    });
    onInvoke("scan_installed_apps", () => [apps[2]]);

    await useUninstallStore.getState().bulkUninstall(apps, false);

    const s = useUninstallStore.getState();
    expect(s.phase).toBe("done");
    expect(s.result).toMatchObject({ items_removed: 2, bytes_freed: 200, errors: ["Figma: Error: busy"] });
    expect(s.apps.map((a) => a.name)).toEqual(["Figma"]);
    expect(invokedWith("add_bytes_freed")).toEqual([{ bytes: 200 }]);
  });

  it("is a no-op for an empty list", async () => {
    await useUninstallStore.getState().bulkUninstall([], false);
    expect(useUninstallStore.getState().phase).toBe("idle");
  });
});
