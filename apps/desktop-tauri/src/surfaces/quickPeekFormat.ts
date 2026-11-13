import type { ProviderUsageSnapshot, RateWindowSnapshot } from "../types/bridge";

export const QUICK_PEEK_PROVIDER_IDS = ["claude", "codex"] as const;

/** The two-lane Windows tray-tooltip text, intentionally without charts. */
export function formatUsagePercent(usedPercent: number): string {
  return Number.isFinite(usedPercent)
    ? String(Math.round(Math.min(100, Math.max(0, usedPercent)))) + "%"
    : "—";
}

export function formatResetCountdown(
  window: RateWindowSnapshot,
  now: Date = new Date(),
): string {
  if (window.resetsAt) {
    const resetTime = Date.parse(window.resetsAt);
    if (Number.isFinite(resetTime)) {
      const minutes = Math.max(0, Math.ceil((resetTime - now.getTime()) / 60_000));
      const days = Math.floor(minutes / (24 * 60));
      const hours = Math.floor((minutes % (24 * 60)) / 60);
      if (days > 0) return days + "d " + hours + "h";
      if (hours > 0) return hours + "h " + (minutes % 60) + "m";
      return minutes > 0 ? minutes + "m" : "now";
    }
  }
  return window.resetDescription?.trim() || "—";
}

export function quickPeekProviders(
  providers: ProviderUsageSnapshot[],
): ProviderUsageSnapshot[] {
  return QUICK_PEEK_PROVIDER_IDS
    .map((id) => providers.find((provider) => provider.providerId === id))
    .filter((provider): provider is ProviderUsageSnapshot => provider !== undefined);
}
