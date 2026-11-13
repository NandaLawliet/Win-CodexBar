import { describe, expect, it } from "vitest";
import type { ProviderUsageSnapshot, RateWindowSnapshot } from "../types/bridge";
import {
  formatResetCountdown,
  formatUsagePercent,
  quickPeekProviders,
} from "./quickPeekFormat";

const now = new Date("2026-11-13T06:00:00.000Z");

function rateWindow(resetsAt: string | null, resetDescription: string | null = null) {
  return { resetsAt, resetDescription } as RateWindowSnapshot;
}

describe("Quick Peek tooltip formatting", () => {
  it("shows bounded whole percentages, not chart metrics", () => {
    expect(formatUsagePercent(22.4)).toBe("22%");
    expect(formatUsagePercent(55.8)).toBe("56%");
    expect(formatUsagePercent(110)).toBe("100%");
    expect(formatUsagePercent(Number.NaN)).toBe("—");
  });

  it("formats short session and longer weekly reset times", () => {
    expect(formatResetCountdown(rateWindow("2026-11-13T08:54:00.000Z"), now))
      .toBe("2h 54m");
    expect(formatResetCountdown(rateWindow("2026-11-16T08:00:00.000Z"), now))
      .toBe("3d 2h");
    expect(formatResetCountdown(rateWindow("2026-11-13T06:00:00.000Z"), now))
      .toBe("now");
  });

  it("retains provider reset text when reset timestamp is absent", () => {
    expect(formatResetCountdown(rateWindow(null, "2h 54m"), now)).toBe("2h 54m");
    expect(formatResetCountdown(rateWindow(null), now)).toBe("—");
  });

  it("filters unrelated providers and orders Claude before Codex", () => {
    const snapshots = ["gemini", "codex", "claude"].map(
      (providerId) => ({ providerId }) as ProviderUsageSnapshot,
    );
    expect(quickPeekProviders(snapshots).map((p) => p.providerId))
      .toEqual(["claude", "codex"]);
  });
});
