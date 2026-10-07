import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import SuccessOverlay, { type SuccessIssue } from "./SuccessOverlay";

const MB = 1024 * 1024;

afterEach(cleanup);

const show = (props: { failedBytes?: number; issues?: SuccessIssue[] }) =>
  render(
    <MemoryRouter>
      <SuccessOverlay headline="All clean" freedGB={5.2} detail="back on your Mac" isPro onDone={() => {}} {...props} />
    </MemoryRouter>,
  );

const npm: SuccessIssue = { label: "npm Cache", path: "/Users/me/.npm/_cacache", size: 13 * MB, reason: "no_permission" };
const slack: SuccessIssue = { label: "Slack Cache", path: "/Users/me/Library/Caches/com.tinyspeck.slackmacgap", size: 2 * MB, reason: "in_use" };
const pip: SuccessIssue = { label: "Pip Cache", path: "/Users/me/Library/Caches/pip", size: 40 * MB, reason: "already_gone" };

describe("SuccessOverlay issues", () => {
  it("shows no issue line when everything was freed", () => {
    show({});
    expect(screen.queryByText(/couldn't be removed/)).toBeNull();
    expect(screen.queryByText(/already gone/)).toBeNull();
  });

  it("says how much couldn't be removed and lists each item with its size and reason", () => {
    show({ failedBytes: 15 * MB, issues: [slack, npm, pip] });
    const toggle = screen.getByRole("button", { name: /15\.0 MB couldn't be removed/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("list")).toBeNull();

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const rows = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(rows).toEqual([
      "npm Cache13.0 MBNo permission",
      "Slack Cache2.0 MBIn use",
      // Already gone is listed, but has no size and isn't part of the total.
      "Pip CacheAlready gone",
    ]);
    expect(screen.getAllByRole("listitem")[0].getAttribute("title")).toBe(npm.path);

    fireEvent.click(toggle);
    expect(screen.queryByRole("list")).toBeNull();
  });

  it("groups several paths of one item into one row", () => {
    const a = { ...npm, path: "/a", size: MB };
    const b = { ...npm, path: "/b", size: 2 * MB };
    show({ failedBytes: 3 * MB, issues: [a, b] });
    fireEvent.click(screen.getByRole("button", { name: /couldn't be removed/ }));
    const [row] = screen.getAllByRole("listitem");
    expect(row.textContent).toBe("npm Cache · 23.0 MBNo permission");
    expect(row.getAttribute("title")).toBe("/a\n/b");
  });

  it("only mentions already-gone items when nothing failed", () => {
    show({ failedBytes: 0, issues: [pip, { ...pip, label: "Homebrew Cache", path: "/h" }] });
    const toggle = screen.getByRole("button", { name: /2 items were already gone/ });
    expect(screen.queryByText(/couldn't be removed/)).toBeNull();
    fireEvent.click(toggle);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });
});
