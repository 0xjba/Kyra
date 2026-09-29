import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { flush, invokedWith, onInvoke } from "../test/tauri";
import { useGuardianStore } from "../stores/guardianStore";
import SubscribeSheet from "./SubscribeSheet";

const initial = useGuardianStore.getState();

beforeEach(() => {
  vi.mocked(openUrl).mockReset();
  vi.mocked(openUrl).mockResolvedValue(undefined);
  onInvoke("guardian_get_device_id", () => "dev-1");
  onInvoke("get_device_name", () => "Mac");
  onInvoke("guardian_check_license", () => ({ active: false, expires: null }));
  onInvoke("guardian_checkout_create", () => ({ short_url: "https://rzp.io/i/abc" }));
});

afterEach(() => {
  cleanup();
  useGuardianStore.getState().stopCheckoutPoll();
  useGuardianStore.setState(initial, true);
});

const typeEmail = (value: string) => fireEvent.change(screen.getByRole("textbox", { name: "Email" }), { target: { value } });
const submit = () => fireEvent.click(screen.getByRole("button", { name: /^Continue/ }));

describe("SubscribeSheet", () => {
  it("renders nothing while closed", () => {
    render(<SubscribeSheet open={false} onClose={() => {}} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("asks for an email and explains why", () => {
    render(<SubscribeSheet open onClose={() => {}} />);
    expect(screen.getByRole("dialog", { name: "Subscribe to Pawtrol" })).toBeTruthy();
    expect(screen.getByText("We'll send your receipt here and use it to restore Pawtrol on another Mac.")).toBeTruthy();
  });

  it("validates the email before creating a checkout", async () => {
    render(<SubscribeSheet open onClose={() => {}} />);
    submit();
    expect(screen.getByRole("alert").textContent).toBe("Enter a valid email address.");
    typeEmail("me@example");
    submit();
    await act(flush);
    expect(screen.getByRole("alert").textContent).toBe("Enter a valid email address.");
    expect(invokedWith("guardian_checkout_create")).toHaveLength(0);
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("opens the hosted checkout, waits for payment and closes once active", async () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    render(<SubscribeSheet open onClose={onClose} />);
    typeEmail("me@example.com");
    submit();
    await act(flush);

    expect(invokedWith("guardian_checkout_create")).toEqual([{ email: "me@example.com" }]);
    expect(openUrl).toHaveBeenCalledWith("https://rzp.io/i/abc");
    expect(screen.getByText("Finish checkout in your browser")).toBeTruthy();
    expect(screen.getByText("Waiting for payment…")).toBeTruthy();

    await act(() => vi.advanceTimersByTimeAsync(5_000));
    expect(invokedWith("guardian_check_license")).toHaveLength(1);
    expect(onClose).not.toHaveBeenCalled();

    onInvoke("guardian_check_license", () => ({ active: true, expires: null }));
    await act(() => vi.advanceTimersByTimeAsync(5_000));
    expect(onClose).toHaveBeenCalled();
    expect(useGuardianStore.getState().checkoutPolling).toBe(false);
  });

  it("keeps the manual refresh and says when payment hasn't landed yet", async () => {
    render(<SubscribeSheet open onClose={() => {}} />);
    typeEmail("me@example.com");
    submit();
    await act(flush);
    fireEvent.click(screen.getByRole("button", { name: "I've subscribed, refresh" }));
    await act(flush);
    expect(invokedWith("guardian_check_license")).toHaveLength(1);
    expect(screen.getByText("Not active yet. It can take a minute after paying.")).toBeTruthy();
  });

  it("shows checkout errors inline and stays on the email step", async () => {
    onInvoke("guardian_checkout_create", () => {
      throw "Couldn't reach Pawtrol. Check your connection and try again.";
    });
    render(<SubscribeSheet open onClose={() => {}} />);
    typeEmail("me@example.com");
    submit();
    await act(flush);
    expect(screen.getByRole("alert").textContent).toBe("Couldn't reach Pawtrol. Check your connection and try again.");
    expect(screen.getByRole("textbox", { name: "Email" })).toBeTruthy();
    expect(openUrl).not.toHaveBeenCalled();
  });
});
