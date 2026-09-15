import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import { useLocale } from "../hooks/useLocale";
import { useProviders } from "../hooks/useProviders";
import {
  getProviderLocalUsageSummary,
  getSettingsSnapshot,
  refreshProvidersIfStale,
} from "../lib/tauri";
import { ProviderIcon } from "../components/providers/ProviderIcon";
import { getProviderIcon } from "../components/providers/providerIcons";
import type {
  BootstrapState,
  ProviderLocalUsageSummary,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
  SettingsSnapshot,
} from "../types/bridge";
import { FLOAT_BAR_CONFIG_CHANGED_EVENT, resizeFloatBar } from "./api";
import "./FloatBar.css";

function ResetIcon({ size }: { size: number }) {
  return (
    <svg
      className="floatbar__reset-icon-svg"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
    >
      <path
        d="M12.9 7.1a5 5 0 1 0-1.2 3.9"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <path
        d="M12.9 3.8v3.3H9.6"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function inlineResetTime(resetText: string): string {
  const normalized = resetText.trim();
  if (/^reset(?:s|ting)?(?:\s+due)?\s*(?:now)?$/i.test(normalized)) {
    return "now";
  }
  return normalized
    .replace(/^resets?\s+in\s+/i, "")
    .replace(/^resets?\s+/i, "")
    .trim();
}

type FloatBarCostSummary = {
  key: string;
  providerId: string;
  displayName: string;
  todayCost: number | null;
  thirtyDayCost: number | null;
};

type FloatBarCostTarget = {
  key: string;
  providerId: string;
  displayName: string;
};

function providerCostKey(provider: ProviderUsageSnapshot): string {
  return `${provider.providerId}:${provider.accountEmail ?? ""}`;
}

function hasLocalCost(summary: ProviderLocalUsageSummary | null): summary is ProviderLocalUsageSummary {
  return summary?.todayCost != null || summary?.thirtyDayCost != null;
}

function formatUsd(value: number | null): string | null {
  if (value == null || !Number.isFinite(value)) return null;
  return `$${value.toFixed(2)}`;
}

function CostPill({
  summary,
  scale,
  todayLabel,
  thirtyDayLabel,
}: {
  summary: FloatBarCostSummary;
  scale: number;
  todayLabel: string;
  thirtyDayLabel: string;
}) {
  const today = formatUsd(summary.todayCost);
  const thirtyDay = formatUsd(summary.thirtyDayCost);
  const iconSize = Math.round(10 * scale);
  const brand = getProviderIcon(summary.providerId).brandColor;
  const title = [
    today ? `${todayLabel} ${today}` : null,
    thirtyDay ? `${thirtyDayLabel} ${thirtyDay}` : null,
  ]
    .filter(Boolean)
    .join(" / ");

  return (
    <div
      className="floatbar__cost-pill"
      title={`${summary.displayName}: ${title}`}
      data-tauri-drag-region
      style={{ "--brand": brand } as CSSProperties}
    >
      <span className="floatbar__provider-icon" data-tauri-drag-region>
        <ProviderIcon providerId={summary.providerId} size={iconSize} />
      </span>
      <span className="floatbar__cost-items" data-tauri-drag-region>
        {today && (
          <span className="floatbar__cost-item" data-tauri-drag-region>
            <span className="floatbar__cost-label" data-tauri-drag-region>
              {todayLabel}
            </span>
            <span className="floatbar__cost-value" data-tauri-drag-region>
              {today}
            </span>
          </span>
        )}
        {thirtyDay && (
          <span className="floatbar__cost-item" data-tauri-drag-region>
            <span className="floatbar__cost-label" data-tauri-drag-region>
              {thirtyDayLabel}
            </span>
            <span className="floatbar__cost-value" data-tauri-drag-region>
              {thirtyDay}
            </span>
          </span>
        )}
      </span>
    </div>
  );
}
// Only these providers own the subscription session/weekly lane contract.
// Informational placeholders must never become fabricated quota percentages.
function quotaWindows(provider: ProviderUsageSnapshot) {
  if (provider.providerId === "zai") {
    // A single Z.ai token window can be either 5-hour or weekly primary.
    const real = [provider.primary, provider.secondary].filter(
      (window): window is RateWindowSnapshot => Boolean(window && !window.isInformational),
    );
    const session = real.find((window) => window.windowMinutes === 300) ?? null;
    const weekly = real.find((window) => window.windowMinutes === 10_080) ?? null;
    return session || weekly ? { session, weekly } : null;
  }
  if (provider.providerId !== "codex" && provider.providerId !== "claude") return null;
  const session = !provider.primary.isInformational &&
    provider.primary.windowMinutes === 300
    ? provider.primary : null;
  const weekly = provider.secondary && !provider.secondary.isInformational &&
    provider.secondary.windowMinutes === 10_080
    ? provider.secondary : null;
  return session || weekly || provider.error ? { session, weekly } : null;
}

type QuotaPresentationProps = {
  provider: ProviderUsageSnapshot;
  highRemaining: number;
  critRemaining: number;
  showAsUsed: boolean;
  scale: number;
  showResetInline: boolean;
  resetRelative: boolean;
  usedSuffix: string;
  remainingSuffix: string;
};

function QuotaMetric({
  rateWindow,
  label,
  provider,
  highRemaining,
  critRemaining,
  showAsUsed,
  scale,
  showResetInline,
  usedSuffix,
  remainingSuffix,
  resetText,
}: QuotaPresentationProps & { rateWindow: RateWindowSnapshot | null; label: string; resetText: string | null }) {
  const remaining = Math.max(0, Math.min(100, rateWindow?.remainingPercent ?? 0));
  const used = Math.max(0, Math.min(100, rateWindow?.usedPercent ?? 0));
  const suffix = showAsUsed ? usedSuffix : remainingSuffix;
  const value = provider.error || !rateWindow ? "—" : `${Math.round(showAsUsed ? used : remaining)}%`;
  const tone = provider.error || rateWindow?.isExhausted || remaining <= critRemaining
    ? "crit" : remaining <= highRemaining ? "warn" : "ok";
  const countdown = resetText ? inlineResetTime(resetText) : null;
  return (
    <div className={`floatbar__quota floatbar__quota--${tone}`} data-tauri-drag-region
      title={`${provider.displayName}: ${label} ${value} ${suffix}${resetText ? `\n${resetText}` : ""}`}>
      <span className="floatbar__text" data-tauri-drag-region>
        <span className="floatbar__quota-label" data-tauri-drag-region>{label}</span>
        <span className="floatbar__pct" data-tauri-drag-region>{value}</span>
      </span>
      {showResetInline && countdown && (
        <span className="floatbar__reset" title={resetText ?? undefined} aria-label={resetText ?? undefined} data-tauri-drag-region>
          <svg width={Math.round(10 * scale)} height={Math.round(10 * scale)} viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <circle cx="8" cy="8" r="6" stroke="currentColor" strokeWidth="1.5" />
            <path d="M8 4v4l2.5 1.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          <span className="floatbar__reset-time" data-tauri-drag-region>{countdown}</span>
        </span>
      )}
    </div>
  );
}

function ProviderPill(props: QuotaPresentationProps) {
  const { t } = useLocale();
  const { provider, scale } = props;
  const windows = quotaWindows(provider);
  const labeled = provider.providerId === "codex" || provider.providerId === "claude" || windows != null;
  const first = labeled ? windows?.session ?? null : provider.selectedMetric;
  const second = windows?.weekly ?? null;
  const firstReset = useFormattedResetTime(first?.resetsAt ?? null, first?.resetDescription ?? null, props.resetRelative);
  const secondReset = useFormattedResetTime(second?.resetsAt ?? null, second?.resetDescription ?? null, props.resetRelative);
  const firstLabel = labeled ? t("ProviderSessionLabel") : "";
  const secondLabel = t("ProviderWeeklyLabel");
  const suffix = props.showAsUsed ? props.usedSuffix : props.remainingSuffix;
  const describe = (window: RateWindowSnapshot | null, label: string, reset: string | null) => {
    const percent = props.showAsUsed ? window?.usedPercent : window?.remainingPercent;
    const value = provider.error || percent == null ? "—" : `${Math.round(Math.max(0, Math.min(100, percent)))}%`;
    return `${label} ${value} ${suffix}${reset ? `\n${reset}` : ""}`.trim();
  };
  const title = `${provider.displayName}: ${[
    first || provider.error ? describe(first, firstLabel, firstReset) : null,
    second ? describe(second, secondLabel, secondReset) : null,
  ].filter(Boolean).join(" / ")}`;
  const brand = getProviderIcon(provider.providerId).brandColor;
  return (
    <div className="floatbar__pill" title={title} data-tauri-drag-region style={{ "--brand": brand } as CSSProperties}>
      <span className="floatbar__identity" data-tauri-drag-region>
        <span className="floatbar__provider-icon" data-tauri-drag-region>
          <ProviderIcon providerId={provider.providerId} size={Math.round(13 * scale)} />
        </span>
        <span data-tauri-drag-region>{provider.displayName}</span>
      </span>
      {first || provider.error ? <QuotaMetric {...props} rateWindow={first} label={firstLabel} resetText={firstReset} /> : <span />}
      {second ? <QuotaMetric {...props} rateWindow={second} label={secondLabel} resetText={secondReset} /> : <span />}
    </div>
  );
}

/**
 * The always-on-top floating capacity bar.
 *
 * Renders stacked provider rows with session and weekly quota columns. Listens to the same provider
 * refresh cycle as the rest of the app via `useProviders`, and reacts to
 * setting changes (filter list, orientation) live without a reload.
 */
export default function FloatBar({ state }: { state: BootstrapState }) {
  const { t } = useLocale();
  const { providers, isRefreshing, refresh: refreshAll } = useProviders({
    refreshOnMount: false,
  });
  const startDrag = useCallback((event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging().catch(() => {});
  }, []);

  // Mark the body so our CSS can strip the dark theme background — the
  // floatbar window is meant to be fully transparent around the pills.
  useEffect(() => {
    document.body.classList.add("floatbar-window");
    return () => {
      document.body.classList.remove("floatbar-window");
    };
  }, []);

  // Local settings: event stream is source of truth after mount; re-seed
  // if the bootstrap prop identity changes (rare parent remount path).
  const [settings, setSettings] = useState(state.settings);
  const [settingsSeed, setSettingsSeed] = useState(state.settings);
  if (state.settings !== settingsSeed) {
    setSettingsSeed(state.settings);
    setSettings(state.settings);
  }
  const [localCosts, setLocalCosts] = useState<Record<string, FloatBarCostSummary>>({});

  // The detached floatbar should keep usage fresh, but it must not open or
  // focus any other surface. Refresh data only; provider-updated events feed
  // this window when the backend completes. Respect Low Power Mode's 30-min
  // floor for automatic ticks (manual refresh remains immediate).
  useEffect(() => {
    const baseMs = Math.max(60_000, settings.refreshIntervalSecs * 1000);
    const intervalMs = settings.lowPowerMode
      ? Math.max(baseMs, 30 * 60 * 1000)
      : baseMs;
    const tick = () => {
      void refreshProvidersIfStale().catch(() => {});
    };
    tick();
    const id = setInterval(tick, intervalMs);
    return () => clearInterval(id);
  }, [settings.refreshIntervalSecs, settings.lowPowerMode]);

  useEffect(() => {
    const unlisten = listen(FLOAT_BAR_CONFIG_CHANGED_EVENT, () => {
      void getSettingsSnapshot().then(setSettings).catch(() => {});
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Orientation flips re-lay-out the bar without recreating the window.
  const orientation: "horizontal" | "vertical" =
    settings.floatBarOrientation === "vertical" ? "vertical" : "horizontal";
  const style = settings.floatBarStyle === "taskbar" ? "taskbar" : "floating";
  const filterIds = settings.floatBarProviderIds;
  const scale = Math.max(0.75, Math.min(2, settings.floatBarScale / 100));
  const showResetInline = settings.floatBarShowResetInline;
  const showCost = settings.floatBarShowCost;
  const visible = useMemo(() => {
    const enabled = new Set(settings.enabledProviders);
    let list = providers.filter((p) => enabled.has(p.providerId));
    if (filterIds && filterIds.length > 0) {
      const wanted = new Set(filterIds);
      list = list.filter((p) => wanted.has(p.providerId));
    }
    return [...list].sort(
      (a, b) =>
        b.selectedMetric.usedPercent - a.selectedMetric.usedPercent,
    );
  }, [providers, settings.enabledProviders, filterIds]);

  const renderableProviders = visible.filter((provider) =>
    provider.providerId === "codex" || provider.providerId === "claude"
      ? quotaWindows(provider) !== null
      : Boolean(provider.error) || !provider.selectedMetric.isInformational,
  );

  const visibleCostTargets = useMemo<FloatBarCostTarget[]>(
    () =>
      showCost
        ? visible.map((provider) => ({
            key: providerCostKey(provider),
            providerId: provider.providerId,
            displayName: provider.displayName,
          }))
        : [],
    [showCost, visible],
  );

  useEffect(() => {
    let cancelled = false;
    const targets = visibleCostTargets;

    if (targets.length === 0) {
      setLocalCosts({});
      return () => {
        cancelled = true;
      };
    }

    Promise.allSettled(
      targets.map(async (target) => {
        const localUsage = await getProviderLocalUsageSummary(target.providerId);
        if (!hasLocalCost(localUsage)) return null;
        return {
          key: target.key,
          providerId: target.providerId,
          displayName: target.displayName,
          todayCost: localUsage.todayCost,
          thirtyDayCost: localUsage.thirtyDayCost,
        } satisfies FloatBarCostSummary;
      }),
    )
      .then((results) => {
        if (cancelled) return;
        const next: Record<string, FloatBarCostSummary> = {};
        for (const result of results) {
          if (result.status === "fulfilled" && result.value) {
            next[result.value.key] = result.value;
          }
        }
        setLocalCosts(next);
      })
      .catch(() => {
        if (!cancelled) setLocalCosts({});
      });

    return () => {
      cancelled = true;
    };
  }, [visibleCostTargets]);

  const visibleCosts = visible
    .map((provider) => localCosts[providerCostKey(provider)])
    .filter((summary): summary is FloatBarCostSummary => Boolean(summary));
  const visibleCostValuesKey = visibleCosts
    .map((summary) => `${summary.key}:${summary.todayCost ?? ""}:${summary.thirtyDayCost ?? ""}`)
    .join("|");
  // Keep the native floatbar window fitted when late data/fonts/icons change layout.
  const lastResizeRef = useRef<{ w: number; h: number } | null>(null);
  const resizeRafRef = useRef<number | null>(null);
  const resizeToContent = useCallback(() => {
    const el = document.querySelector<HTMLElement>(".floatbar");
    if (!el) return;
    if (resizeRafRef.current !== null) {
      cancelAnimationFrame(resizeRafRef.current);
    }
    resizeRafRef.current = requestAnimationFrame(() => {
      resizeRafRef.current = null;
      const rect = el.getBoundingClientRect();
      const padding = 8;
      const dpr =
        Number.isFinite(window.devicePixelRatio) && window.devicePixelRatio > 0
          ? window.devicePixelRatio
          : 1;
      // DOM measurements are CSS pixels; the native command accepts physical
      // pixels so the window remains correctly sized on scaled displays.
      const w = Math.ceil(Math.ceil(rect.width + padding) * dpr);
      const h = Math.ceil(Math.ceil(rect.height + padding) * dpr);
      const last = lastResizeRef.current;
      if (last && Math.abs(last.w - w) <= 1 && Math.abs(last.h - h) <= 1) return;
      lastResizeRef.current = { w, h };
      void resizeFloatBar(w, h).catch(() => {});
    });
  }, []);

  useEffect(() => {
    resizeToContent();
  }, [
    resizeToContent,
    visible.length,
    visibleCostValuesKey,
    orientation,
    style,
    scale,
    showResetInline,
    settings.resetTimeRelative,
  ]);

  useEffect(() => {
    const el = document.querySelector<HTMLElement>(".floatbar");
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(resizeToContent);
    observer.observe(el);
    return () => observer.disconnect();
  }, [resizeToContent]);

  useEffect(() => {
    // Re-measure after moving onto a monitor with a different scale factor.
    window.addEventListener("resize", resizeToContent);
    return () => window.removeEventListener("resize", resizeToContent);
  }, [resizeToContent]);

  useEffect(
    () => () => {
      if (resizeRafRef.current !== null) {
        cancelAnimationFrame(resizeRafRef.current);
      }
    },
    [],
  );

  const highRemaining = 100 - settings.highUsageThreshold;
  const critRemaining = 100 - settings.criticalUsageThreshold;
  const opacityFraction = Math.max(0.3, Math.min(1, settings.floatBarOpacity / 100));

  return (
    <div
      role="group"
      aria-label={t("AppName")}
      className={`floatbar floatbar--${orientation} floatbar--${style}${settings.floatBarDarkText ? " floatbar--light-bg" : ""}`}
      data-tauri-drag-region
      onMouseDown={startDrag}
      style={
        {
          opacity: opacityFraction,
          "--floatbar-scale": scale,
        } as CSSProperties
      }
    >
      <div className="floatbar__handle" data-tauri-drag-region aria-hidden />
      {renderableProviders.length === 0 && visibleCosts.length === 0 ? (
        <div className="floatbar__empty" data-tauri-drag-region>
          {t("FloatBarNoProviders")}
        </div>
      ) : (
        <>
          <div className="floatbar__providers" data-tauri-drag-region>
            {renderableProviders.map((p) => (
              <ProviderPill
                key={providerCostKey(p)}
                provider={p}
                highRemaining={highRemaining}
                critRemaining={critRemaining}
                showAsUsed={settings.showAsUsed}
                scale={scale}
                showResetInline={showResetInline}
                resetRelative={settings.resetTimeRelative}
                usedSuffix={t("PanelUsedSuffix")}
                remainingSuffix={t("FloatBarRemainingSuffix")}
              />
            ))}
          </div>
        </>
      )}
      {visibleCosts.map((summary) => (
        <CostPill
          key={`cost:${summary.key}`}
          summary={summary}
          scale={scale}
          todayLabel={t("PanelToday")}
          thirtyDayLabel={t("FloatBarThirtyDayShort")}
        />
      ))}
      {!settings.floatBarClickThrough && <button type="button" className="floatbar__refresh" disabled={isRefreshing}
        aria-label={t("ActionRefreshAll")} title={t("ActionRefreshAll")} aria-busy={isRefreshing}
        onPointerDown={(event) => event.stopPropagation()}
        onMouseDown={(event) => event.stopPropagation()}
        onClick={() => void refreshAll()}>
        <ResetIcon size={Math.round(12 * scale)} />
        <span>{isRefreshing ? t("SummaryRefreshing") : t("ActionRefreshAll")}</span>
      </button>}
    </div>
  );
}
