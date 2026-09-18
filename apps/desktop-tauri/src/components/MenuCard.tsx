import { type CSSProperties, useCallback, useEffect, useState } from "react";
import type {
  CostSummaryDisplayStyle,
  ProviderChartData,
  ProviderUsageSnapshot,
} from "../types/bridge";
import { getProviderChartData } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import { formatRelativeUpdated } from "../lib/relativeTime";
import type { LocaleKey } from "../i18n/keys";
import { providerSupportsChartData } from "../lib/providerCharts";
import MenuCardDetails, { describeCard, type MetricEntry } from "./MenuCardDetails";
import CodexAccountsMenu from "./CodexAccountsMenu";
import { DEEPSEEK_PRICING_EVENT } from "../hooks/useDeepSeekPricingStatus";
import { getDeepSeekPricingStatus } from "../lib/tauri";
import type { DeepSeekPricingStatus } from "../types/bridge";

/** Small copy-to-clipboard button matching macOS CopyIconButton (doc.on.doc → checkmark). */
function CopyIconButton({ text }: { text: string }) {
  const { t } = useLocale();
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 900);
  }, [text]);
  return (
    <button
      type="button"
      className="menu-card__copy-btn"
      onClick={handleCopy}
      aria-label={copied ? t("PanelCopied") : t("ActionCopyError")}
      title={copied ? t("PanelCopied") : t("ActionCopyError")}
    >
      {copied ? "✓" : (
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
          <rect x="5" y="5" width="9" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.5"/>
          <path d="M11 3V2.5A1.5 1.5 0 009.5 1H2.5A1.5 1.5 0 001 2.5v7A1.5 1.5 0 002.5 11H3" stroke="currentColor" strokeWidth="1.5"/>
        </svg>
      )}
    </button>
  );
}

export interface MenuCardDisplayOptions {
  hideEmail: boolean;
  resetTimeRelative: boolean;
  showResetWhenExhausted?: boolean;
  showPace?: boolean;
  showAsUsed?: boolean;
  compactMetrics?: boolean;
  costSummaryDisplayStyle?: CostSummaryDisplayStyle;
}

interface MenuCardProps {
  provider: ProviderUsageSnapshot;
  display: MenuCardDisplayOptions;
  isRefreshing?: boolean;
  /** Per-provider accent color override (hex); applied as CSS --provider-accent. */
  accentColor?: string;
  onLayoutChange?: () => void;
}


export function maskEmail(email: string): string {
  const at = email.indexOf("@");
  if (at <= 1) return "••••@••••";
  return email[0] + "•".repeat(at - 1) + email.slice(at);
}

/** Localize raw provider window labels using the active locale. */
function localizeWindowLabel(
  raw: string | undefined,
  t: (key: LocaleKey) => string,
  language?: string,
  windowMinutes?: number | null,
): string {
  const normalized = raw?.trim().toLowerCase();
  // Upstream 0.55.0 #3070: quota windows in Simplified Chinese use their
  // actual duration instead of the conversational Session wording.
  if (language === "chinese" && normalized === "session" && windowMinutes != null) {
    if (windowMinutes === 7 * 24 * 60) return t("ProviderWeeklyLabel");
    if (windowMinutes >= 60 && windowMinutes <= 12 * 60 && windowMinutes % 60 === 0) {
      return `${windowMinutes / 60} 小时`;
    }
  }
  if (normalized === "weekly") {
    return t("ProviderWeeklyLabel");
  }
  // F5 (upstream 0.48.0): monthly (30-day) window label.
  if (normalized === "monthly") {
    return t("ProviderMonthly");
  }
  return raw ?? "";
}

function displayPlanName(
  planName: string | null,
  t: (key: LocaleKey) => string,
): string | null {
  if (!planName) return null;
  const normalized = planName.trim().toLowerCase();
  if (normalized === "default_claude_ai") return t("ProviderPlanClaudeAi");
  return planName;
}

/**
 * Provider card — direct mirror of SwiftUI `UsageMenuCardView`.
 *
 * Layout (top to bottom):
 *   1. Header VStack(spacing: 3)
 *        – HStack: providerName (headline/semibold)  ··  email (subheadline/secondary, right)
 *        – HStack: subtitle "source · updated"        ··  plan (footnote/secondary, right)
 *   2. Divider (1pt)
 *   3. VStack(spacing: 12)
 *        – Metrics group VStack(spacing: 12) of MetricRow
 *        – (Divider) Cost group: title (body/medium) + session line + month line (footnote)
 *        – (Divider) Pace group (Tauri-only addition; placed last)
 *        – (Divider) Charts group (Tauri-only addition; placed last)
 *
 * Padding: upstream v0.32.2 uses wider horizontal card padding and slightly
 * taller header/content vertical padding so account/plan rows can breathe.
 */
/**
 * Map Antigravity live provider data into four semantic lanes:
 * 1. Gemini 5h (cadence 300)
 * 2. Gemini Weekly (cadence 10080)
 * 3. Claude/GPT 5h (cadence 300)
 * 4. Claude/GPT Weekly (cadence 10080)
 *
 * Uses semantic metadata (titles, IDs, cadence) rather than array indices.
 * Never fabricates missing lanes. Never reuses legacy labels "Claude" / "Gemini Pro".
 */
export function getAntigravityMetrics(provider: ProviderUsageSnapshot): MetricEntry[] {
  const extras = provider.extraRateWindows ?? [];

  const is5h = (mins: number | null | undefined, title: string, id: string) =>
    mins === 300 || /\b5h\b|5-hour|session/i.test(title) || /5h|session/i.test(id);

  const isWeekly = (mins: number | null | undefined, title: string, id: string) =>
    mins === 10080 || /weekly/i.test(title) || /weekly/i.test(id);

  const isGemini = (title: string, id: string) =>
    /gemini/i.test(title) || /gemini/i.test(id);

  const isClaudeGpt = (title: string, id: string) =>
    /claude|gpt|third-party/i.test(title) || /claude|gpt|third-party/i.test(id);

  const findExtra = (
    familyMatch: (title: string, id: string) => boolean,
    cadenceMatch: (mins: number | null | undefined, title: string, id: string) => boolean,
  ) => {
    return extras.find((entry) => {
      const t = entry.title;
      const i = entry.id;
      return familyMatch(t, i) && cadenceMatch(entry.window.windowMinutes, t, i);
    });
  };

  const gemini5hExtra = findExtra(isGemini, is5h);
  const geminiWeeklyExtra = findExtra(isGemini, isWeekly);
  const claudeGpt5hExtra = findExtra(isClaudeGpt, is5h);
  const claudeGptWeeklyExtra = findExtra(isClaudeGpt, isWeekly);

  const result: MetricEntry[] = [];

  if (gemini5hExtra) {
    result.push({
      id: gemini5hExtra.id,
      label: "Gemini 5h",
      snap: gemini5hExtra.window,
    });
  } else if (
    provider.primary &&
    provider.primary.windowMinutes === 300 &&
    !/claude/i.test(provider.primaryLabel ?? "")
  ) {
    result.push({
      id: "gemini-5h",
      label: "Gemini 5h",
      snap: provider.primary,
    });
  }

  if (geminiWeeklyExtra) {
    result.push({
      id: geminiWeeklyExtra.id,
      label: "Gemini Weekly",
      snap: geminiWeeklyExtra.window,
    });
  } else if (
    provider.secondary &&
    provider.secondary.windowMinutes === 10080 &&
    !/gemini pro/i.test(provider.secondaryLabel ?? "")
  ) {
    result.push({
      id: "gemini-weekly",
      label: "Gemini Weekly",
      snap: provider.secondary,
    });
  }

  if (claudeGpt5hExtra) {
    result.push({
      id: claudeGpt5hExtra.id,
      label: "Claude/GPT 5h",
      snap: claudeGpt5hExtra.window,
    });
  }

  if (claudeGptWeeklyExtra) {
    result.push({
      id: claudeGptWeeklyExtra.id,
      label: "Claude/GPT Weekly",
      snap: claudeGptWeeklyExtra.window,
    });
  }

  return result;
}

export default function MenuCard({
  provider,
  display,
  isRefreshing = false,
  accentColor,
  onLayoutChange,
}: MenuCardProps) {
  const {
    hideEmail,
    resetTimeRelative,
    showResetWhenExhausted = false,
    showPace = true,
    showAsUsed = false,
    compactMetrics = false,
    costSummaryDisplayStyle,
  } = display;
  const { t, language } = useLocale();
  const [chartData, setChartData] = useState<ProviderChartData | null>(null);
  const [pricingStatus, setPricingStatus] = useState<DeepSeekPricingStatus | null>(null);

  useEffect(() => {
    if (provider.providerId !== "deepseek") return;
    const onPricing = (event: Event) =>
      setPricingStatus((event as CustomEvent<DeepSeekPricingStatus>).detail);
    window.addEventListener(DEEPSEEK_PRICING_EVENT, onPricing);
    void getDeepSeekPricingStatus().then(setPricingStatus).catch(() => {});
    return () => window.removeEventListener(DEEPSEEK_PRICING_EVENT, onPricing);
  }, [provider.providerId]);

  useEffect(() => {
    if (!providerSupportsChartData(provider.providerId)) {
      setChartData(null);
      return;
    }
    let cancelled = false;
    setChartData(null);
    getProviderChartData(
      provider.providerId,
      provider.accountEmail ?? undefined,
    )
      .then((data) => {
        if (!cancelled) {
          setChartData(data);
          requestAnimationFrame(() => onLayoutChange?.());
        }
      })
      .catch(() => {
        /* chart data is best-effort */
      });
    return () => {
      cancelled = true;
    };
  }, [provider.providerId, provider.accountEmail, onLayoutChange]);

  const isWayfinder = provider.providerId === "wayfinder";
  const email = !isWayfinder && provider.accountEmail
    ? hideEmail
      ? maskEmail(provider.accountEmail)
      : provider.accountEmail
    : null;
  const planName = !isWayfinder ? displayPlanName(provider.planName, t) : null;

  const metrics: MetricEntry[] = [
    ...(isWayfinder
      ? []
      : [
          {
            id: "primary",
            label: localizeWindowLabel(provider.primaryLabel, t, language, provider.primary.windowMinutes) || t("DetailWindowPrimary"),
            snap: provider.primary,
          },
        ]),
  ];
  if (provider.secondary)
    metrics.push({
      id: "secondary",
      label: localizeWindowLabel(provider.secondaryLabel, t) || t("DetailWindowSecondary"),
      snap: provider.secondary,
      sessionEquivalentForecast: provider.sessionEquivalentForecast,
    });
  if (provider.modelSpecific)
    metrics.push({
      id: "model-specific",
      label: t("DetailWindowModelSpecific"),
      snap: provider.modelSpecific,
    });
  if (provider.tertiary)
    metrics.push({
      id: "tertiary",
      // F5 (upstream 0.48.0): use the cadence-based label (e.g. "Monthly") instead
      // of the generic "DetailWindowTertiary" slot key when tertiaryLabel is set.
      label: localizeWindowLabel(provider.tertiaryLabel, t) || t("DetailWindowTertiary"),
      snap: provider.tertiary,
    });
  for (const extra of provider.extraRateWindows ?? []) {
    metrics.push({
      id: `extra-${extra.id}`,
      label: extra.title,
      snap: extra.window,
      resetFormatMode: extra.id === "reset-credits" ? "expires" : "reset",
    });
  }

  const isAntigravity = provider.providerId === "antigravity";
  const antigravityMetrics = isAntigravity ? getAntigravityMetrics(provider) : [];
  const visibleMetrics = isAntigravity
    ? antigravityMetrics
    : compactMetrics
      ? metrics.slice(0, 2)
      : metrics;

  const presence = describeCard(
    provider,
    chartData,
    visibleMetrics,
    costSummaryDisplayStyle,
    showPace,
  );
  const { hasDetails } = presence;
  const cardClassName = [
    "menu-card",
    provider.error ? "menu-card--error" : null,
    isRefreshing ? "menu-card--refreshing" : null,
    hasDetails ? "menu-card--with-details" : "menu-card--header-only",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article
      className={cardClassName}
      aria-busy={isRefreshing}
      style={accentColor ? ({ "--provider-accent": accentColor } as CSSProperties) : undefined}
    >
      <header className="menu-card__header">
        <div className="menu-card__title-row">
          <div className="menu-card__name-group">
            <span className="menu-card__name">{provider.displayName}</span>
            {!provider.error && email && <span className="menu-card__email">{email}</span>}
          </div>
        </div>
        {provider.error ? (
          <div className="menu-card__error-block">
            <div className="menu-card__error-text">{provider.error}</div>
            <CopyIconButton text={provider.error} />
          </div>
        ) : (
          <div className="menu-card__subtitle-row">
            <span className="menu-card__subtitle">
              {Number.isNaN(Date.parse(provider.updatedAt))
                ? provider.updatedAt
                : formatRelativeUpdated(Date.parse(provider.updatedAt), t)}
            </span>
            {planName && (
              <span className="menu-card__plan-badge">{planName}</span>
            )}
          </div>
        )}
      </header>

      {hasDetails && <div className="menu-card__divider" />}

      {hasDetails && (
        <MenuCardDetails
          provider={provider}
          display={{
            resetTimeRelative,
            showResetWhenExhausted,
            showPace,
            showAsUsed,
            costSummaryDisplayStyle,
          }}
          metrics={visibleMetrics}
          chartData={chartData}
          presence={presence}
          onLayoutChange={onLayoutChange}
        />
      )}

      {provider.providerId === "deepseek" && pricingStatus && (
        <section
          className="menu-card__pricing-status"
          aria-label={t("DeepSeekPricingTitle")}
        >
          <strong>
            {t("DeepSeekPricingTitle")}: {t(
              pricingStatus.period === "peak"
                ? "DeepSeekPricingPeak"
                : pricingStatus.period === "offPeak"
                  ? "DeepSeekPricingOffPeak"
                  : "DeepSeekPricingStandard",
            )}
          </strong>
          <span>
            {t("DeepSeekPricingCurrent")} {pricingStatus.currentLocalTime}
          </span>
          <span>
            {t("DeepSeekPricingNext")} {pricingStatus.nextTransitionLocalTime ?? "—"}
          </span>
          <span>
            {t("DeepSeekPricingEffective")} {pricingStatus.effectiveLocalTime}
          </span>
          <small>{t("DeepSeekPricingAdvice")}</small>
        </section>
      )}

      {provider.providerId === "codex" && (
        <CodexAccountsMenu hideEmail={hideEmail} />
      )}
    </article>
  );
}
