import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { flush, invokedWith, onInvoke } from "../test/tauri";
import { useGuardianStore } from "../stores/guardianStore";
import RestoreSheet from "./RestoreSheet";

const initial = useGuardianStore.getState();

beforeEach(() => {
  onInvoke("guardian_restore_start", () => undefined);
  onInvoke("guardian_restore_verify", () => ({ active: true, expires: 1_932_854_400 }));
});

afterEach(() => {
  cleanup();
  useGuardianStore.setState(initial, true);
});

async function toCodeStep(email = "me@example.com") {
  fireEvent.change(screen.getByRole("textbox", { name: "Email" }), { target: { value: email } });
  fireEvent.click(screen.getByRole("button", { name: "Send code" }));
  await act(flush);
}

const codeField = () => screen.getByRole("textbox", { name: "6-digit code" }) as HTMLInputElement;
const enterCode = async (value: string) => {
  fireEvent.change(codeField(), { target: { value } });
  await act(flush);
};

describe("RestoreSheet", () => {
  it("validates the email before sending a code", async () => {
    render(<RestoreSheet open onClose={() => {}} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Email" }), { target: { value: "nope" } });
    fireEvent.click(screen.getByRole("button", { name: "Send code" }));
    await act(flush);
    expect(screen.getByRole("alert").textContent).toBe("Enter a valid email address.");
    expect(invokedWith("guardian_restore_start")).toHaveLength(0);
  });

  it("emails a code, then verifies a pasted code and shows success", async () => {
    const onClose = vi.fn();
    render(<RestoreSheet open onClose={onClose} />);
    await toCodeStep(" me@example.com ");

    expect(invokedWith("guardian_restore_start")).toEqual([{ email: "me@example.com" }]);
    expect(screen.getByText("me@example.com")).toBeTruthy();

    await enterCode("Code: 123 456");
    expect(invokedWith("guardian_restore_verify")).toEqual([{ email: "me@example.com", code: "123456" }]);
    expect(screen.getByText("Pawtrol is back on this Mac")).toBeTruthy();
    expect(useGuardianStore.getState().license.active).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("shows a wrong code inline, clears the field and lets you try again", async () => {
    let attempts = 0;
    onInvoke("guardian_restore_verify", () => {
      if (++attempts === 1) throw "That code didn't work. Check it and try again.";
      return { active: true, expires: null };
    });
    render(<RestoreSheet open onClose={() => {}} />);
    await toCodeStep();

    await enterCode("12345");
    expect(invokedWith("guardian_restore_verify")).toHaveLength(0);
    expect((screen.getByRole("button", { name: "Verify" }) as HTMLButtonElement).disabled).toBe(true);

    await enterCode("000000");
    expect(screen.getByRole("alert").textContent).toBe("That code didn't work. Check it and try again.");
    expect(codeField().value).toBe("");
    expect(useGuardianStore.getState().license.active).toBe(false);

    await enterCode("654321");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("Pawtrol is back on this Mac")).toBeTruthy();
  });

  it("explains when the email has no active subscription", async () => {
    onInvoke("guardian_restore_verify", () => ({ active: false, expires: null }));
    render(<RestoreSheet open onClose={() => {}} />);
    await toCodeStep();
    await enterCode("123456");
    expect(screen.getByRole("alert").textContent).toBe(
      "We couldn't find an active Pawtrol subscription for that email.",
    );
  });

  it("allows a resend only after 30 seconds", async () => {
    vi.useFakeTimers();
    render(<RestoreSheet open onClose={() => {}} />);
    await toCodeStep();

    const resend = () => screen.getByRole("button", { name: /Resend code/ }) as HTMLButtonElement;
    expect(resend().textContent).toBe("Resend code in 0:30");
    expect(resend().disabled).toBe(true);

    for (let i = 0; i < 29; i++) await act(() => vi.advanceTimersByTimeAsync(1_000));
    expect(resend().textContent).toBe("Resend code in 0:01");
    await act(() => vi.advanceTimersByTimeAsync(1_000));
    expect(resend().textContent).toBe("Resend code");
    expect(resend().disabled).toBe(false);

    fireEvent.click(resend());
    await act(flush);
    expect(invokedWith("guardian_restore_start")).toHaveLength(2);
    expect(screen.getByText("We sent a new code.")).toBeTruthy();
    expect(resend().disabled).toBe(true);
  });

  it("shows send errors inline", async () => {
    onInvoke("guardian_restore_start", () => {
      throw "Too many codes sent. Try again in an hour.";
    });
    render(<RestoreSheet open onClose={() => {}} />);
    await toCodeStep();
    expect(screen.getByRole("alert").textContent).toBe("Too many codes sent. Try again in an hour.");
    expect(screen.queryByRole("textbox", { name: "6-digit code" })).toBeNull();
  });
});
