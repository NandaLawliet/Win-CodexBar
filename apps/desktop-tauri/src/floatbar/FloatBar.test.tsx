import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getCachedProviders: vi.fn(),
  getProviderChartData: vi.fn(),
  getProviderLocalUsageSummary: vi.fn(),
  refreshProviders: vi.fn(),
  refreshProvidersIfStale: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  updateSettings: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listeners: new Map<string, Array<(event: { payload: unknown }) => void>>(),
}));

const windowMocks = vi.hoisted(() => ({
  getCurrentWindow: vi.fn(() => ({
    startDragging: vi.fn().mockResolvedValue(undefined),
  })),
}));

const coreMocks = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/api/event", () => eventMocks);
vi.mock("@tauri-apps/api/window", () => windowMocks);
vi.mock("@tauri-apps/api/core", () => coreMocks);

import FloatBar from "./FloatBar";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import type {
  BootstrapState,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
  SettingsSnapshot,
} from "../types/bridge";

type RateWindowOptions = {
  exhausted?: boolean;
  informational?: boolean;
  resetsAt?: string | null;
  resetDescription?: string | null;
};

function rateWindow(
  used: number,
  opts: RateWindowOptions = {},
): RateWindowSnapshot {
  return {
    usedPercent: used,
    remainingPercent: 100 - used,
    windowMinutes: null,
    resetsAt: opts.resetsAt ?? null,
    resetDescription: opts.resetDescription ?? null,
    isExhausted: opts.exhausted ?? false,
    isInformational: opts.informational,
    reservePercent: null,
    reserveDescription: null,
  };
}

function snapshot(
  id: string,
  display: string,
  used: number,
  opts: {
    exhausted?: boolean;
    error?: string | null;
    resetsAt?: string | null;
    resetDescription?: string | null;
    informational?: boolean;
    secondary?: {
      used: number;
      exhausted?: boolean;
      informational?: boolean;
      resetsAt?: string | null;
      resetDescription?: string | null;
    };
    selected?: {
      used: number;
      exhausted?: boolean;
      informational?: boolean;
      resetsAt?: string | null;
      resetDescription?: string | null;
    };
  } = {},
): ProviderUsageSnapshot {
  const primary = rateWindow(used, opts);
  const secondary = opts.secondary
    ? rateWindow(opts.secondary.used, opts.secondary)
    : null;
  primary.windowMinutes = 300;
  if (secondary) secondary.windowMinutes = 10_080;
  const selectedMetric = opts.selected
    ? rateWindow(opts.selected.used, opts.selected)
    : primary.isInformational && secondary && !secondary.isInformational
      ? secondary
      : primary;

  return {
    primaryLabel: id === "claude" ? "Session (5h)" : "Session",
    secondaryLabel: "Weekly",
    providerId: id,
    displayName: display,
    primary,
    selectedMetric,
    secondary,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "auto",
    updatedAt: "2026-05-15T00:00:00Z",
    error: opts.error ?? null,
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
  };
}

function zaiSnapshot(): ProviderUsageSnapshot {
  const provider = snapshot("zai", "z.ai", 0, {
    resetDescription: "5-hour",
    secondary: { used: 98.6, resetDescription: "Weekly" },
  });
  delete provider.primaryLabel;
  delete provider.secondaryLabel;
  provider.selectedMetric = provider.secondary!;
  return provider;
}

function settings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
  return {
    enabledProviders: ["claude", "codex"],
    refreshIntervalSecs: 300,
    adaptiveRefresh: false,
    refreshAllProvidersOnMenuOpen: false,
    lowPowerMode: false,
    startAtLogin: false,
    startMinimized: false,
    showNotifications: true,
    soundEnabled: true,
    notificationSoundTheme: "windows",
    notificationSoundPaths: {
      predictiveWarning: null,
      highUsage: null,
      criticalUsage: null,
      exhausted: null,
      statusIssue: null,
      sessionDepleted: null,
      sessionRestored: null,
    },
    highUsageThreshold: 70,
    criticalUsageThreshold: 90,
    predictivePaceWarningEnabled: false,
    trayIconMode: "single",
    switcherShowsIcons: true,
    menuBarShowsHighestUsage: false,
    menuBarShowsPercent: false,
    showAsUsed: true,
    showAllTokenAccountsInMenu: false,
    enableAnimations: true,
    resetTimeRelative: true,
    showResetWhenExhausted: false,
    menuBarDisplayMode: "detailed",
    hidePersonalInfo: false,
    updateChannel: "stable",
    autoDownloadUpdates: false,
    installUpdatesOnQuit: false,
    globalShortcut: "Ctrl+Shift+U",
    codexCustomSessionsDirs: [],
    uiLanguage: "english",
    theme: "dark",
    windowScalePercent: 125,
    trayScalePercent: 100,
    powertoysStatusPipeEnabled: false,
    claudeAvoidKeychainPrompts: false,
    codexSparkUsageVisible: true,
    disableKeychainAccess: false,
    providerMetrics: {},
    floatBarEnabled: true,
    floatBarOpacity: 80,
    floatBarScale: 100,
    floatBarOrientation: "horizontal",
    floatBarStyle: "floating",
    floatBarClickThrough: false,
    floatBarProviderIds: [],
    floatBarDarkText: false,
    floatBarShowResetInline: false,
    floatBarShowCost: false,
    claudeDailyRoutinesUsageVisible: true,
    claudeAllowReadingClaudeCodeCredentials: false,
    alibabaTokenPlanRegion: "cn",
    weeklyProgressWorkDays: null,
    costSummaryDisplayStyle: "compact",
    providerAccentColors: {},
    ...overrides,
  };
}

function bootstrap(settingsOverrides: Partial<SettingsSnapshot> = {}): BootstrapState {
  return {
    contractVersion: "v1",
    providers: [],
    settings: settings(settingsOverrides),
  };
}

function renderFloatBar(state: BootstrapState) {
  return render(
    <LocaleProvider>
      <FloatBar state={state} />
    </LocaleProvider>,
  );
}

describe("FloatBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.refreshProviders.mockResolvedValue(undefined);
    tauriMocks.refreshProvidersIfStale.mockResolvedValue(undefined);
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue(null);
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ResetsInHoursMinutes: "Resets in {}h {}m",
        ResetsInDaysHours: "Resets in {}d {}h",
        TrayResetsDueNow: "Resetting",
        ProviderSessionLabel: "Session",
        ProviderWeeklyLabel: "Weekly",
        PanelToday: "Today",
        ActionRefreshAll: "Refresh all",
        SummaryRefreshing: "Refreshing",
        PanelUsedSuffix: "used",
        FloatBarThirtyDayShort: "30d",
        FloatBarNoProviders: "No providers",
        FloatBarRemainingSuffix: "remaining",
      }),
    );
    eventMocks.listeners.clear();
    eventMocks.listen.mockImplementation((event: string, handler: (event: { payload: unknown }) => void) => {
      const listeners = eventMocks.listeners.get(event) ?? [];
      listeners.push(handler);
      eventMocks.listeners.set(event, listeners);
      return Promise.resolve(() => {});
    });
  });

  it("keeps the horizontal layout contract unwrapped with independent pill grids", () => {
    const floatBarCss = readFileSync("src/floatbar/FloatBar.css", "utf8");
    expect(floatBarCss).toMatch(
      /\.floatbar--horizontal \.floatbar__providers\s*\{[^}]*display:\s*inline-flex;[^}]*flex-flow:\s*row nowrap;/s,
    );
    expect(floatBarCss).toMatch(
      /\.floatbar--horizontal \.floatbar__pill\s*\{[^}]*grid-column:\s*auto;[^}]*grid-template-columns:\s*max-content max-content max-content;/s,
    );
    expect(floatBarCss).toMatch(
      /\.floatbar--horizontal \.floatbar__refresh\s*\{[^}]*order:\s*-1;/s,
    );
  });

  it("hot-updates orientation without remounting or reordering providers", async () => {
    const enabledProviders = ["zai", "codex", "claude"];
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20),
      snapshot("codex", "Codex", 75),
      zaiSnapshot(),
    ]);

    const { container } = renderFloatBar(
      bootstrap({ enabledProviders, floatBarOrientation: "horizontal" }),
    );
    await waitFor(() => {
      expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(3);
      expect(eventMocks.listeners.get("float-bar-config-changed")).toHaveLength(1);
    });

    const bar = container.querySelector(".floatbar");
    const providerOrder = () =>
      Array.from(container.querySelectorAll(".floatbar__identity")).map((row) =>
        row.textContent?.trim(),
      );
    expect(bar).toHaveClass("floatbar--horizontal");
    expect(bar).not.toHaveClass("floatbar--vertical");
    expect(providerOrder()).toEqual(["z.ai", "Codex", "Claude"]);

    const emitConfigChanged = () => {
      for (const listener of eventMocks.listeners.get("float-bar-config-changed") ?? []) {
        listener({ payload: {} });
      }
    };
    tauriMocks.getSettingsSnapshot.mockResolvedValueOnce(
      settings({ enabledProviders, floatBarOrientation: "vertical" }),
    );
    act(emitConfigChanged);
    await waitFor(() => expect(bar).toHaveClass("floatbar--vertical"));
    expect(container.querySelector(".floatbar")).toBe(bar);
    expect(providerOrder()).toEqual(["z.ai", "Codex", "Claude"]);

    tauriMocks.getSettingsSnapshot.mockResolvedValueOnce(
      settings({ enabledProviders, floatBarOrientation: "horizontal" }),
    );
    act(emitConfigChanged);
    await waitFor(() => expect(bar).toHaveClass("floatbar--horizontal"));
    expect(container.querySelector(".floatbar")).toBe(bar);
    expect(providerOrder()).toEqual(["z.ai", "Codex", "Claude"]);
  });

  it("renders a pill per enabled provider, sorted by usage descending", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20),
      snapshot("codex", "Codex", 75),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowCost: true }),
    );

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      const pills = container.querySelectorAll(".floatbar__pill");
      expect(pills.length).toBe(2);
    });

    const titles = Array.from(container.querySelectorAll(".floatbar__quota")).map(
      (el) => el.getAttribute("title") ?? "",
    );
    // Highest used (codex, 75%) shows first; display follows showAsUsed.
    expect(titles[0]).toMatch(/Codex: Session 75% used/);
    expect(titles[1]).toMatch(/Claude: Session 20% used/);
  });

  it("renders session even when a weekly window is available", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20, { secondary: { used: 90 } }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ providerMetrics: { claude: "session" } }),
    );

    const { container } = renderFloatBar(
      bootstrap({ providerMetrics: { claude: "session" } }),
    );
    await waitFor(() => {
      expect(container.querySelector(".floatbar__quota")?.getAttribute("title")).toContain(
        "Claude: Session 20% used",
      );
    });
  });

  it("shows only real weekly data when the session is informational", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 0, {
        informational: true,
        secondary: { used: 37 },
        selected: { used: 37 },
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ providerMetrics: { codex: "weekly" } }),
    );

    const { container } = renderFloatBar(
      bootstrap({ providerMetrics: { codex: "weekly" } }),
    );
    await waitFor(() => {
      expect(container.querySelector(".floatbar__quota")?.getAttribute("title")).toContain(
        "Codex: Weekly 37% used",
      );
    });
  });

  it("uses a real secondary window when the primary window is informational", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 10, {
        informational: true,
        secondary: {
          used: 80,
          resetsAt: null,
          resetDescription: "Resets in 2 hours",
        },
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowResetInline: true }),
    );

    const { container } = renderFloatBar(bootstrap({ floatBarShowResetInline: true }));
    await waitFor(() => {
      const pill = container.querySelector(".floatbar__quota");
      expect(pill?.getAttribute("title")).toContain("Claude: Weekly 80% used\nResets in 2 hours");
      expect(pill?.classList.contains("floatbar__quota--warn")).toBe(true);
      expect(container.querySelector(".floatbar__reset")?.textContent).toContain("2 hours");
    });
  });

  it("omits informational placeholders when neither quota is real", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 10, { informational: true }),
      snapshot("codex", "Codex", 10, { informational: true, secondary: { used: 90, informational: true } }),
    ]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(tauriMocks.getCachedProviders).toHaveBeenCalled());
    expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(0);
  });

  it("keeps the existing provider ordering while showing both actual windows", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 90, { secondary: { used: 20 }, selected: { used: 20 } }),
      snapshot("codex", "Codex", 50),
    ]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(container.querySelectorAll(".floatbar__identity")).toHaveLength(2));
    expect(Array.from(container.querySelectorAll(".floatbar__identity")).map((row) => row.textContent?.trim())).toEqual(["Codex", "Claude"]);
    expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(3);
  });

  it("loads local cost summaries without using the foreground chart endpoint", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 75),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue({
      todayCost: 1.25,
      thirtyDayCost: 12.5,
      thirtyDayTokens: 1000,
      latestTokens: 200,
      topModel: "gpt-5",
      estimateNote: "Estimated from local logs",
      tokenCostUpdatedAtMs: 1234,
    });

    renderFloatBar(bootstrap({ floatBarShowCost: true }));

    await waitFor(() => {
      expect(tauriMocks.getProviderLocalUsageSummary).toHaveBeenCalledWith("codex");
    });
    expect(tauriMocks.getProviderChartData).not.toHaveBeenCalled();
  });

  it("does not claim no providers while a real local cost is renderable", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("codex", "Codex", 0, { informational: true })]);
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue({ todayCost: 1.25, thirtyDayCost: 12.5 });
    const { container } = renderFloatBar(bootstrap({ floatBarShowCost: true }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__cost-pill")).toHaveLength(1));
    expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(0);
    expect(screen.queryByText("No providers")).not.toBeInTheDocument();
  });

  it("does not scan local costs by default", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 75),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    renderFloatBar(bootstrap());

    await waitFor(() => {
      expect(tauriMocks.getCachedProviders).toHaveBeenCalled();
    });
    expect(tauriMocks.getProviderLocalUsageSummary).not.toHaveBeenCalled();
  });

  it("can show remaining percentages when configured", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ showAsUsed: false }));

    const { container } = renderFloatBar(bootstrap({ showAsUsed: false }));

    await waitFor(() => {
      const title = container
        .querySelector(".floatbar__quota")
        ?.getAttribute("title");
      expect(title).toContain("Claude: Session 80% remaining");
    });
  });

  it("applies warning tone when remaining drops below the high threshold", async () => {
    // highUsageThreshold = 70 → high-remaining cutoff = 30%.
    // claude at 80% used → 20% remaining → critical (also below crit cutoff 10).
    // Use 75% used → 25% remaining → warn (between 10 and 30).
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("claude", "Claude", 75)]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      expect(container.querySelector(".floatbar__quota--warn")).not.toBeNull();
    });
  });

  it("applies critical tone when the provider is exhausted", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 100, { exhausted: true }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      expect(container.querySelector(".floatbar__quota--crit")).not.toBeNull();
    });
  });

  it("filters to the floatBarProviderIds allowlist when non-empty", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 30),
      snapshot("codex", "Codex", 50),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarProviderIds: ["codex"] }),
    );

    const { container } = renderFloatBar(
      bootstrap({ floatBarProviderIds: ["codex"] }),
    );
    await waitFor(() => {
      const pills = container.querySelectorAll(".floatbar__pill");
      expect(pills.length).toBe(1);
      expect(pills[0].textContent).toMatch(/Codex/);
    });
  });

  it("does not show stale cached providers when all providers are disabled", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 30),
      snapshot("codex", "Codex", 50),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ enabledProviders: [] }),
    );

    const { container } = renderFloatBar(bootstrap({ enabledProviders: [] }));
    await waitFor(() => {
      expect(container.querySelectorAll(".floatbar__pill").length).toBe(0);
      expect(container.querySelector(".floatbar__empty")).not.toBeNull();
    });
  });

  it("shows an empty state when no providers match", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      expect(container.querySelector(".floatbar__empty")).not.toBeNull();
    });
  });

  it("applies the light-background class and CSS opacity", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarDarkText: true, floatBarOpacity: 45 }),
    );

    const { container } = renderFloatBar(
      bootstrap({ floatBarDarkText: true, floatBarOpacity: 45 }),
    );

    await waitFor(() => {
      const bar = container.querySelector<HTMLElement>(".floatbar");
      expect(bar).not.toBeNull();
      expect(bar?.classList.contains("floatbar--light-bg")).toBe(true);
      expect(bar?.style.opacity).toBe("0.45");
    });
  });

  it("applies the configured scale as a CSS variable", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ floatBarScale: 150 }));

    const { container } = renderFloatBar(bootstrap({ floatBarScale: 150 }));

    await waitFor(() => {
      const bar = container.querySelector<HTMLElement>(".floatbar");
      expect(bar).not.toBeNull();
      expect(bar?.style.getPropertyValue("--floatbar-scale")).toBe("1.5");
    });
  });

  it("resizes the native window in physical pixels at the WebView DPI", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
    const originalDevicePixelRatio = window.devicePixelRatio;
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 1.5,
    });
    const rectSpy = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockReturnValue({
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        top: 0,
        right: 100,
        bottom: 20,
        left: 0,
        toJSON: () => ({}),
      });

    try {
      renderFloatBar(bootstrap());

      await waitFor(() => {
        expect(coreMocks.invoke).toHaveBeenCalledWith("resize_float_bar", {
          width: 162,
          height: 42,
        });
      });
    } finally {
      rectSpy.mockRestore();
      Object.defineProperty(window, "devicePixelRatio", {
        configurable: true,
        value: originalDevicePixelRatio,
      });
    }
  });

  it("uses the localized reset formatter in pill tooltips", async () => {
    const resetsAt = new Date(Date.now() + 3 * 60 * 60_000 + 42 * 60_000).toISOString();
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20, { resetsAt }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());

    await waitFor(() => {
      const title = container
        .querySelector(".floatbar__quota")
        ?.getAttribute("title");
      expect(title).toContain("Claude: Session 20% used");
      expect(title).toMatch(/Resets in 3h 4[12]m/);
      expect(title).not.toContain("Resets in due now");
    });
  });

  it("can render a next reset icon and time in provider pills", async () => {
    const resetsAt = new Date(Date.now() + 2 * 60 * 60_000 + 5 * 60_000).toISOString();
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20, { resetsAt }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowResetInline: true }),
    );

    const { container } = renderFloatBar(
      bootstrap({ floatBarShowResetInline: true }),
    );

    await waitFor(() => {
      const reset = container.querySelector(".floatbar__reset");
      expect(reset).not.toBeNull();
      expect(reset?.getAttribute("aria-label")).toMatch(/Resets in 2h [45]m/);
      expect(reset?.textContent).toMatch(/2h [45]m/);
      expect(reset?.textContent).not.toContain("Resets in");
    });
  });

  it("polls refreshProvidersIfStale on the configured interval", async () => {
    vi.useFakeTimers();
    try {
      tauriMocks.getCachedProviders.mockResolvedValue([]);
      tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
      // 60s minimum is enforced in FloatBar.tsx; use the floor here.
      await act(async () => {
        renderFloatBar(bootstrap({ refreshIntervalSecs: 60 }));
      });

      // Initial tick fires synchronously on mount; useProviders is passive here
      // so the floatbar does not double-request stale refreshes at startup.
      await vi.waitFor(() => {
        expect(tauriMocks.refreshProvidersIfStale).toHaveBeenCalledTimes(1);
      });
      const initialCalls = tauriMocks.refreshProvidersIfStale.mock.calls.length;

      // Advance the timer past the 60-second interval — the floatbar tick
      // should fire again.
      await vi.advanceTimersByTimeAsync(60_000);
      expect(tauriMocks.refreshProvidersIfStale.mock.calls.length).toBeGreaterThan(
        initialCalls,
      );
    } finally {
      vi.useRealTimers();
    }
  });
  it.each([true, false])("renders session left of weekly with independent values (showAsUsed=%s)", async (showAsUsed) => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 96, { secondary: { used: 24 }, selected: { used: 24 } }),
      snapshot("claude", "Claude", 25, { secondary: { used: 40 } }),
    ]);
    const { container } = renderFloatBar(bootstrap({ showAsUsed }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(2));
    const codex = Array.from(container.querySelectorAll(".floatbar__pill")).find((row) => row.textContent?.includes("Codex"))!;
    const quotas = codex.querySelectorAll(".floatbar__quota");
    expect(quotas).toHaveLength(2);
    expect(quotas[0].textContent).toBe(`Session${showAsUsed ? 96 : 4}%`);
    expect(quotas[1].textContent).toBe(`Weekly${showAsUsed ? 24 : 76}%`);
    expect(quotas[0]).toHaveClass("floatbar__quota--crit");
    expect(quotas[1]).toHaveClass("floatbar__quota--ok");
    expect(codex).not.toHaveClass("floatbar__quota--crit");
  });

  it("uses each window's own reset and hides only countdowns when disabled", async () => {
    const weeklyReset = new Date(Date.now() + 6 * 24 * 60 * 60_000 + 14 * 60 * 60_000).toISOString();
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 25, {
        resetDescription: "Resets in 4h57m",
        secondary: { used: 24, resetsAt: weeklyReset, resetDescription: "Wrong description" },
      }),
    ]);
    const view = renderFloatBar(bootstrap({ floatBarShowResetInline: true, resetTimeRelative: true }));
    await waitFor(() => expect(view.container.querySelectorAll(".floatbar__reset")).toHaveLength(2));
    const resets = view.container.querySelectorAll(".floatbar__reset");
    expect(resets[0].textContent).toBe("4h57m");
    expect(resets[1].textContent).toMatch(/6d 1[34]h/);
    view.rerender(<LocaleProvider><FloatBar state={bootstrap({ floatBarShowResetInline: false })} /></LocaleProvider>);
    expect(view.container.querySelectorAll(".floatbar__reset")).toHaveLength(0);
    expect(view.container.querySelectorAll(".floatbar__quota")).toHaveLength(2);
    expect(view.container.querySelectorAll(".floatbar__pct")[1].textContent).toBe("24%");
  });

  it("uses semantic durations even when supporting labels differ", async () => {
    const actual = snapshot("claude", "Claude", 20, { secondary: { used: 30 } });
    actual.primaryLabel = "Tokens";
    actual.secondaryLabel = "MCP";
    tauriMocks.getCachedProviders.mockResolvedValue([actual]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(2));
    expect(container.querySelectorAll(".floatbar__quota")[0].textContent).toBe("Session20%");
    expect(container.querySelectorAll(".floatbar__quota")[1].textContent).toBe("Weekly30%");
  });

  it("renders production Claude session and weekly labels", async () => {
    const claude = snapshot("claude", "Claude", 25, { secondary: { used: 40 } });
    expect(claude.primaryLabel).toBe("Session (5h)");
    expect(claude.secondaryLabel).toBe("Weekly");
    tauriMocks.getCachedProviders.mockResolvedValue([claude]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(2));
    expect(container.querySelectorAll(".floatbar__quota")[0].textContent).toBe("Session25%");
    expect(container.querySelectorAll(".floatbar__quota")[1].textContent).toBe("Weekly40%");
  });

  it("does not fabricate a missing weekly lane", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("claude", "Claude", 25)]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(1));
    expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(1);
    expect(screen.queryByText("Weekly")).not.toBeInTheDocument();
  });

  it.each([false, true])("keeps Claude errors visible with unavailable quota (placeholder=%s)", async (placeholder) => {
    const claude = snapshot("claude", "Claude", 0, { error: "Unavailable", informational: placeholder,
      secondary: placeholder ? undefined : { used: 0 } });
    if (placeholder) claude.primary.windowMinutes = null;
    tauriMocks.getCachedProviders.mockResolvedValue([claude]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(1));
    expect(screen.getByText("Claude")).toBeVisible();
    expect(container.querySelectorAll(".floatbar__pct")).toHaveLength(placeholder ? 1 : 2);
    for (const value of container.querySelectorAll(".floatbar__pct")) expect(value.textContent).toBe("—");
    if (placeholder) expect(screen.queryByText("Weekly")).not.toBeInTheDocument();
  });

  it.each(["gemini", "copilot", "cursor", "zai"])("keeps %s selected-metric fallback without fake quota labels", async (id) => {
    const provider = snapshot(id, id, 20, { selected: { used: 64, resetDescription: "Resets tomorrow" } });
    if (id === "zai") provider.primary.windowMinutes = null;
    provider.primaryLabel = "Tokens";
    tauriMocks.getCachedProviders.mockResolvedValue([provider]);
    const { container } = renderFloatBar(bootstrap({ enabledProviders: [id] }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(1));
    expect(container.querySelector(".floatbar__pct")?.textContent).toBe("64%");
    expect(container.querySelector(".floatbar__pill")).toHaveAttribute("title", `${id}: 64% used\nResets tomorrow`);
    expect(screen.queryByText("Weekly")).not.toBeInTheDocument();
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
    expect(screen.queryByText("No providers")).not.toBeInTheDocument();
  });

  it.each([true, false])("renders Z.ai semantic lanes with independent values and tones (showAsUsed=%s)", async (showAsUsed) => {
    tauriMocks.getCachedProviders.mockResolvedValue([zaiSnapshot()]);
    const { container } = renderFloatBar(bootstrap({ enabledProviders: ["zai"], showAsUsed }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(2));
    const quotas = container.querySelectorAll(".floatbar__quota");
    expect(quotas[0].textContent).toBe(`Session${showAsUsed ? 0 : 100}%`);
    expect(quotas[1].textContent).toBe(`Weekly${showAsUsed ? 99 : 1}%`);
    expect(quotas[0]).toHaveClass("floatbar__quota--ok");
    expect(quotas[1]).toHaveClass("floatbar__quota--crit");
    expect(container.querySelector(".floatbar__pill")).not.toHaveClass("floatbar__quota--crit");
  });

  it.each(["timestamp", "description"])("associates Z.ai resets with their own lanes (%s)", async (source) => {
    const provider = zaiSnapshot();
    if (source === "timestamp") {
      provider.primary.resetsAt = new Date(Date.now() + (4 * 60 + 30) * 60_000).toISOString();
      provider.secondary!.resetsAt = new Date(Date.now() + (24 + 17) * 60 * 60_000).toISOString();
    } else {
      provider.primary.resetDescription = "Resets in 4h 30m";
      provider.secondary!.resetDescription = "Resets in 1d 17h";
    }
    tauriMocks.getCachedProviders.mockResolvedValue([provider]);
    const { container } = renderFloatBar(bootstrap({ enabledProviders: ["zai"], floatBarShowResetInline: true }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__reset")).toHaveLength(2));
    const quotas = container.querySelectorAll(".floatbar__quota");
    expect(quotas[0].querySelector(".floatbar__reset-time")?.textContent).toMatch(/4h (29|30)m/);
    expect(quotas[1].querySelector(".floatbar__reset-time")?.textContent).toMatch(/1d (16|17)h/);
    expect(quotas[0].getAttribute("title")).toContain("Session 0% used\nResets in 4h");
    expect(quotas[1].getAttribute("title")).toContain("Weekly 99% used\nResets in 1d");
    const title = container.querySelector(".floatbar__pill")?.getAttribute("title");
    expect(title).toContain(quotas[0].getAttribute("title")!.replace("z.ai: ", ""));
    expect(title).toContain(quotas[1].getAttribute("title")!.replace("z.ai: ", ""));
  });

  it.each([300, 10_080])("does not fabricate Z.ai's missing lane with a single %s-minute token window", async (duration) => {
    const provider = zaiSnapshot();
    provider.primary = duration === 300 ? provider.primary : provider.secondary!;
    provider.secondary = null;
    provider.selectedMetric = provider.primary;
    tauriMocks.getCachedProviders.mockResolvedValue([provider]);
    const { container } = renderFloatBar(bootstrap({ enabledProviders: ["zai"] }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(1));
    expect(container.querySelector(".floatbar__quota")?.textContent).toBe(duration === 300 ? "Session0%" : "Weekly99%");
    expect(screen.queryByText(duration === 300 ? "Weekly" : "Session")).not.toBeInTheDocument();
  });

  it("keeps Z.ai unrelated durations on the truthful selected-metric fallback", async () => {
    const provider = zaiSnapshot();
    provider.primary.windowMinutes = 60;
    provider.secondary!.windowMinutes = 43_200;
    tauriMocks.getCachedProviders.mockResolvedValue([provider]);
    const { container } = renderFloatBar(bootstrap({ enabledProviders: ["zai"] }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(1));
    expect(container.querySelector(".floatbar__quota")?.textContent).toBe("99%");
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
    expect(screen.queryByText("Weekly")).not.toBeInTheDocument();
  });

  it.each(["session", "weekly", "both"])("does not promote Z.ai informational quota placeholders (%s)", async (placeholder) => {
    const provider = zaiSnapshot();
    provider.primary.isInformational = placeholder !== "weekly";
    provider.secondary!.isInformational = placeholder !== "session";
    provider.selectedMetric = provider.primary.isInformational ? provider.secondary! : provider.primary;
    tauriMocks.getCachedProviders.mockResolvedValue([provider]);
    const { container } = renderFloatBar(bootstrap({ enabledProviders: ["zai"] }));
    if (placeholder === "both") {
      await screen.findByText("No providers");
      expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(0);
    } else {
      await waitFor(() => expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(1));
      expect(container.querySelector(".floatbar__quota")?.textContent).toBe(placeholder === "session" ? "Weekly99%" : "Session0%");
      expect(screen.queryByText(placeholder === "session" ? "Session" : "Weekly")).not.toBeInTheDocument();
    }
  });

  it("keeps reset tooltip on the hit-testable pill with inline reset disabled", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("claude", "Claude", 25, {
      resetDescription: "Resets in 2h", secondary: { used: 40, resetDescription: "Resets in 6d" },
    })]);
    const { container } = renderFloatBar(bootstrap({ floatBarShowResetInline: false }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(1));
    expect(container.querySelector(".floatbar__pill")).toHaveAttribute("title", "Claude: Session 25% used\nResets in 2h / Weekly 40% used\nResets in 6d");
    expect(container.querySelectorAll(".floatbar__reset")).toHaveLength(0);
    expect(container.querySelector(".floatbar__pill")).toHaveAttribute("data-tauri-drag-region");
  });

  it("honors absolute reset formatting", async () => {
    const reset = new Date(Date.now() + 24 * 60 * 60_000).toISOString();
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("claude", "Claude", 25, { resetsAt: reset })]);
    const { container } = renderFloatBar(bootstrap({ resetTimeRelative: false, floatBarShowResetInline: true }));
    const expected = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(new Date(reset));
    await waitFor(() => expect(container.querySelector(".floatbar__reset-time")?.textContent).toBe(expected));
    expect(container.querySelector(".floatbar__pill")?.getAttribute("title")).toContain(expected);
  });

  it("hides Refresh All in click-through mode", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("codex", "Codex", 20)]);
    const { container } = renderFloatBar(bootstrap({ floatBarClickThrough: true }));
    await waitFor(() => expect(container.querySelectorAll(".floatbar__pill")).toHaveLength(1));
    expect(screen.queryByRole("button", { name: "Refresh all" })).not.toBeInTheDocument();
  });

  it("rejects unrelated window durations even for an agent provider", async () => {
    const unrelated = snapshot("codex", "Codex", 20, { secondary: { used: 30 } });
    unrelated.primary.windowMinutes = 60;
    unrelated.secondary!.windowMinutes = 43_200;
    tauriMocks.getCachedProviders.mockResolvedValue([unrelated]);
    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => expect(tauriMocks.getCachedProviders).toHaveBeenCalled());
    expect(container.querySelectorAll(".floatbar__quota")).toHaveLength(0);
  });

  it("refreshes immediately without overlapping clicks or dragging", async () => {
    let complete!: () => void;
    tauriMocks.refreshProviders.mockImplementation(() => new Promise<void>((resolve) => { complete = resolve; }));
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    renderFloatBar(bootstrap());
    const button = await screen.findByRole("button", { name: "Refresh all" });
    windowMocks.getCurrentWindow.mockClear();
    fireEvent.pointerDown(button);
    fireEvent.mouseDown(button, { button: 0 });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(tauriMocks.refreshProviders).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).not.toHaveAttribute("data-tauri-drag-region");
    expect(windowMocks.getCurrentWindow).not.toHaveBeenCalled();
    await act(async () => complete());
    expect(button).toBeEnabled();
    fireEvent.mouseDown(document.querySelector(".floatbar__handle")!, { button: 0 });
    expect(windowMocks.getCurrentWindow).toHaveBeenCalledTimes(1);
  });

  it("disables manual refresh throughout an event-driven automatic cycle", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    renderFloatBar(bootstrap());
    const button = await screen.findByRole("button", { name: "Refresh all" });
    expect(eventMocks.listeners.get("refresh-started")).toHaveLength(1);
    expect(eventMocks.listeners.get("refresh-complete")).toHaveLength(1);
    const emit = (name: string, payload: unknown = {}) => {
      for (const listener of eventMocks.listeners.get(name) ?? []) listener({ payload });
    };
    act(() => emit("refresh-started", { providerIds: ["codex"] }));
    fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(tauriMocks.refreshProviders).not.toHaveBeenCalled();
    // A provider update does not complete the global cycle.
    act(() => emit("provider-updated", snapshot("codex", "Codex", 20)));
    expect(button).toBeDisabled();
    act(() => emit("refresh-complete", { providerCount: 1, errorCount: 0 }));
    expect(button).toBeEnabled();
  });

  it.each([false, true])("recovers the refresh button after command failure (started=%s)", async (started) => {
    tauriMocks.getCachedProviders.mockResolvedValue([]);
    tauriMocks.refreshProviders.mockImplementation(async () => {
      if (started) {
        for (const listener of eventMocks.listeners.get("refresh-started") ?? []) {
          listener({ payload: { providerIds: ["codex"] } });
        }
      }
      throw new Error("command unavailable");
    });
    renderFloatBar(bootstrap());
    const button = await screen.findByRole("button", { name: "Refresh all" });
    fireEvent.click(button);
    await waitFor(() => expect(button).toBeEnabled());
    expect(tauriMocks.refreshProviders).toHaveBeenCalledTimes(1);
  });

  it("keeps the low-power automatic cadence and vertical orientation", async () => {
    vi.useFakeTimers();
    try {
      tauriMocks.getCachedProviders.mockResolvedValue([]);
      await act(async () => { renderFloatBar(bootstrap({ refreshIntervalSecs: 60, lowPowerMode: true, floatBarOrientation: "vertical", floatBarStyle: "taskbar" })); });
      expect(document.querySelector(".floatbar")).toHaveClass("floatbar--vertical", "floatbar--taskbar");
      await vi.advanceTimersByTimeAsync(60_000);
      expect(tauriMocks.refreshProvidersIfStale).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(29 * 60_000);
      expect(tauriMocks.refreshProvidersIfStale).toHaveBeenCalledTimes(2);
      expect(tauriMocks.refreshProviders).not.toHaveBeenCalled();
    } finally { vi.useRealTimers(); }
  });

  it("observes wider quota content and remeasures after a DPI change", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("codex", "Codex", 40, { secondary: { used: 25 } })]);
    let resize!: ResizeObserverCallback;
    const observe = vi.fn();
    const disconnect = vi.fn();
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { resize = callback; }
      observe = observe;
      disconnect = disconnect;
    });
    let width = 420;
    const rectSpy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({
      x: 0, y: 0, width, height: 80, top: 0, right: width, bottom: 80, left: 0, toJSON: () => ({}),
    }));
    const originalDpr = window.devicePixelRatio;
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 1 });
    try {
      const view = renderFloatBar(bootstrap());
      await waitFor(() => expect(coreMocks.invoke).toHaveBeenCalledWith("resize_float_bar", { width: 428, height: 88 }));
      expect(observe).toHaveBeenCalledWith(view.container.querySelector(".floatbar"));
      width = 640;
      act(() => resize([], {} as ResizeObserver));
      await waitFor(() => expect(coreMocks.invoke).toHaveBeenCalledWith("resize_float_bar", { width: 648, height: 88 }));
      Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 1.5 });
      fireEvent(window, new Event("resize"));
      await waitFor(() => expect(coreMocks.invoke).toHaveBeenCalledWith("resize_float_bar", { width: 972, height: 132 }));
      view.unmount();
      expect(disconnect).toHaveBeenCalled();
    } finally {
      rectSpy.mockRestore();
      vi.unstubAllGlobals();
      Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: originalDpr });
    }
  });

});
