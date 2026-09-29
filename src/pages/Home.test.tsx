import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import Home from "./Home";
import { useSummaryStore } from "../stores/summaryStore";

const GB = 1024 * 1024 * 1024;
const at = Date.now();

afterEach(() => {
  cleanup();
  useSummaryStore.setState({ cleans: 0, clean: undefined, prune: undefined, installers: undefined, optimizedAt: undefined, firstCleanAt: undefined });
});

function renderHome() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/prune" element={<div>prune page</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Home", () => {
  it("renders tiles from the seeded summary store", () => {
    useSummaryStore.setState({
      cleans: 3,
      prune: { bytes: 2 * GB, count: 4, projects: 2, at },
      installers: { bytes: 0, count: 0, at },
      optimizedAt: at,
    });
    renderHome();

    expect(screen.getByText("Build artifacts in 2 projects")).toBeTruthy();
    expect(screen.getByText("Tidy")).toBeTruthy();
    expect(screen.getByText("No installers found")).toBeTruthy();
    expect(screen.getByText("System caches & logs")).toBeTruthy();
    expect(screen.getByText("Today")).toBeTruthy();
    expect(screen.getAllByText("2.0 GB").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "Review it all" }));
    expect(screen.getByText("prune page")).toBeTruthy();
  });

  it("asks for a first scan when there are no summaries", () => {
    renderHome();
    expect(screen.getByRole("button", { name: "Start scanning" })).toBeTruthy();
    expect(screen.getByText("No tune-up yet")).toBeTruthy();
  });
});
