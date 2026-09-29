import { describe, expect, it } from "vitest";
import { formatSize } from "./format";

const KB = 1024;
const MB = 1024 * KB;
const GB = 1024 * MB;

describe("formatSize", () => {
  it("shows raw bytes below 1 KB", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1023)).toBe("1023 B");
  });

  it("shows whole kilobytes", () => {
    expect(formatSize(KB)).toBe("1 KB");
    expect(formatSize(500 * KB)).toBe("500 KB");
  });

  it("switches to MB at 1000 KB so labels never read four-digit KB", () => {
    expect(formatSize(999 * KB)).toBe("999 KB");
    expect(formatSize(1000 * KB)).toBe("1.0 MB");
    expect(formatSize(250 * MB)).toBe("250.0 MB");
  });

  it("switches to GB at 1000 MB", () => {
    expect(formatSize(999 * MB)).toBe("999.0 MB");
    expect(formatSize(1000 * MB)).toBe("1.0 GB");
    expect(formatSize(12.34 * GB)).toBe("12.3 GB");
  });
});
