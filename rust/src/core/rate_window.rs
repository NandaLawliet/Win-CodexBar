//! Rate window model - represents a usage limit window (e.g., 5-hour session, 7-day weekly)

use super::session_equivalent_forecast::{
    MONTHLY_WINDOW_MINUTES, SESSION_WINDOW_MINUTES, WEEKLY_WINDOW_MINUTES,
};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

/// Duration cadence of a rate-limit window (upstream 0.48.0 F5).
///
/// Codex exposes three lanes: a 5-hour session, a 7-day weekly, and a 30-day
/// monthly window. Centralizing the cadence here keeps duration-first labels
/// and bucketing consistent across the ambient provider, the managed multi-
/// account stack, and the CLI/tray surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateWindowCadence {
    /// 5-hour session window (300 minutes).
    Session,
    /// 7-day weekly window (10 080 minutes).
    Weekly,
    /// 30-day monthly window (43 200 minutes).
    Monthly,
    /// Any other window length the upstream buckets don't recognize.
    Unknown,
}

impl RateWindowCadence {
    /// Classify a window length given in minutes into a cadence.
    ///
    /// Bucketing (upstream `RateLane` + `classifyRateWindow`):
    /// - exactly 300 → Session
    /// - 43 200+    → Monthly
    /// - 10 080..<43 200 → Weekly
    /// - anything else → Unknown
    pub fn from_minutes(minutes: u32) -> Self {
        if minutes == SESSION_WINDOW_MINUTES {
            Self::Session
        } else if minutes >= MONTHLY_WINDOW_MINUTES {
            Self::Monthly
        } else if minutes >= WEEKLY_WINDOW_MINUTES {
            Self::Weekly
        } else {
            Self::Unknown
        }
    }

    /// Classify from seconds (the API and `UsageWindowSnapshot` report seconds).
    pub fn from_seconds(seconds: i64) -> Self {
        if seconds <= 0 {
            return Self::Unknown;
        }
        Self::from_minutes(((seconds + 59) / 60) as u32)
    }

    /// Human-readable label key for this cadence (matches upstream lane names).
    pub fn label_key(&self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Unknown => "unknown",
        }
    }
}

/// Represents a rate limit window with usage percentage and reset time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateWindow {
    /// Percentage of the window that has been used (0-100)
    pub used_percent: f64,

    /// Duration of the window in minutes (e.g., 300 for 5-hour, 10080 for 7-day)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u32>,

    /// When the window resets
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,

    /// Human-readable reset description (e.g., "Jan 15 at 3:00pm")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_description: Option<String>,

    /// Whether this row is an informational value rather than a quota.
    #[serde(default)]
    pub is_informational: bool,

    /// Whether `used_percent` came from a real, in-process upstream quota
    /// reading — **in-memory provenance only, never serialized**.
    ///
    /// # Fail-closed invariant
    ///
    /// `#[serde(skip)]` means this flag is neither written to nor read from
    /// JSON. A `RateWindow` rebuilt from any serialized snapshot therefore
    /// comes back **non-authoritative** (`false`), and security-sensitive
    /// consumers must treat it as unavailable rather than as a healthy quota.
    /// Authority is something a constructor grants from a live upstream value;
    /// it is never inferred from a byte stream.
    ///
    /// This deliberately differs from [`NamedRateWindow::usage_known`], which
    /// defaults to `true` on deserialize: that field is presentation metadata,
    /// this one is quota authority, so the two must not share a default.
    ///
    /// Read it through [`RateWindow::authoritative_used_percent`] rather than
    /// directly — the flag alone is not sufficient proof of a usable quota.
    ///
    /// [`NamedRateWindow::usage_known`]: crate::core::NamedRateWindow::usage_known
    #[serde(skip)]
    pub quota_authoritative: bool,
}

impl RateWindow {
    /// Create a new rate window
    pub fn new(used_percent: f64) -> Self {
        let (used_percent, quota_authoritative) = Self::normalize_percent(used_percent);
        Self {
            used_percent,
            window_minutes: None,
            resets_at: None,
            reset_description: None,
            is_informational: false,
            quota_authoritative,
        }
    }

    /// Create an informational row without implying a percentage quota.
    pub fn informational(description: impl Into<String>) -> Self {
        Self {
            reset_description: Some(description.into()),
            is_informational: true,
            ..Self::new(0.0).non_authoritative()
        }
    }

    /// Informational placeholder for an absent 5-hour session lane.
    ///
    /// Weekly-only plans (Codex, Claude web) occasionally report no active
    /// session window. Informational primaries are omitted from quota math
    /// and rendered as unavailable, so one canonical shape keeps providers
    /// and downstream surfaces from diverging.
    pub fn no_active_session() -> Self {
        Self {
            window_minutes: Some(SESSION_WINDOW_MINUTES),
            ..Self::informational("No active 5h session")
        }
    }

    /// Create a rate window with full details
    pub fn with_details(
        used_percent: f64,
        window_minutes: Option<u32>,
        resets_at: Option<DateTime<Utc>>,
        reset_description: Option<String>,
    ) -> Self {
        let (used_percent, quota_authoritative) = Self::normalize_percent(used_percent);
        Self {
            used_percent,
            window_minutes,
            resets_at,
            reset_description,
            is_informational: false,
            quota_authoritative,
        }
    }

    /// Record whether the caller's own provenance supports quota authority.
    ///
    /// Authority is **granted only at construction**, from a finite, in-range
    /// upstream value. This builder can only ever *narrow* it: passing `true`
    /// keeps whatever the constructor already established, it never re-grants
    /// it. That matters because a rejected input is stored as a `0.0`
    /// placeholder, which is numerically indistinguishable from a genuine 0% —
    /// so laundering must be impossible by construction, not by revalidating
    /// the stored number.
    ///
    /// The usual shape is `RateWindow::with_details(..).with_quota_authority(
    /// parsed.is_some())`: valid parse keeps authority, missing parse drops it.
    #[must_use]
    pub fn with_quota_authority(mut self, authoritative: bool) -> Self {
        self.quota_authoritative &= authoritative;
        self
    }

    /// Mark this window as fabricated, missing, or unparseable quota.
    ///
    /// Use this at every site that invents a percentage the provider did not
    /// actually report (`unwrap_or(0.0)` fallbacks, cost-only pseudo-windows,
    /// "no limits configured" placeholders). The numeric value and the public
    /// JSON stay exactly as they were; only the security provenance drops.
    #[must_use]
    pub fn non_authoritative(self) -> Self {
        self.with_quota_authority(false)
    }

    /// The one fail-closed accessor for security-sensitive quota decisions.
    ///
    /// Returns `Some(used_percent)` only when **every** condition holds:
    ///
    /// - quota authority was explicitly established in-process
    ///   ([`Self::quota_authoritative`]),
    /// - the row is a real quota, not an informational placeholder,
    /// - `used_percent` is finite,
    /// - `used_percent` is within the inclusive range `[0, 100]`,
    /// - `window_minutes` is present,
    /// - and that duration classifies as `expected` under
    ///   [`RateWindowCadence::from_minutes`].
    ///
    /// Anything else — including a window that declares a *different* cadence
    /// than the caller asked for — yields `None`. The check inspects only
    /// `self`: it never scans sibling windows, never promotes another lane, and
    /// contains no provider-specific branching.
    pub fn authoritative_used_percent(&self, expected: RateWindowCadence) -> Option<f64> {
        let used_percent = self.trusted_used_percent()?;
        let minutes = self.window_minutes?;
        (RateWindowCadence::from_minutes(minutes) == expected).then_some(used_percent)
    }

    /// Cadence-agnostic core of [`Self::authoritative_used_percent`].
    ///
    /// Applies every authority gate except the cadence match, for the one
    /// consumer whose contract selects windows by snapshot slot rather than by
    /// declared duration (see `cli::guard`). Prefer
    /// [`Self::authoritative_used_percent`] whenever an expected cadence is
    /// known.
    pub fn trusted_used_percent(&self) -> Option<f64> {
        if !self.quota_authoritative || self.is_informational {
            return None;
        }
        Self::percent_is_trustworthy(self.used_percent).then_some(self.used_percent)
    }

    /// Whether a percent value may back an authoritative quota reading.
    ///
    /// Finite and inside the inclusive `[0, 100]` band. Non-finite values and
    /// out-of-range values are rejected even after clamping, because the
    /// clamped number is a fabrication rather than a measurement.
    pub(crate) fn percent_is_trustworthy(value: f64) -> bool {
        value.is_finite() && (0.0..=100.0).contains(&value)
    }

    /// Normalize a constructor input into `(stored_percent, authoritative)`.
    ///
    /// - Valid `[0, 100]` inputs are stored bit-for-bit and stay authoritative.
    /// - Finite out-of-range inputs keep the historical clamp but lose
    ///   authority: a clamped number is not a measurement.
    /// - Non-finite inputs are **never retained** — NaN and ±Infinity serialize
    ///   to JSON `null` (breaking the `f64` schema) and read as "plenty of
    ///   headroom" in every naive comparison. They collapse to a safe `0.0`
    ///   placeholder that is simultaneously marked non-authoritative, so no
    ///   security-sensitive accessor can ever return it.
    fn normalize_percent(value: f64) -> (f64, bool) {
        if !value.is_finite() {
            (0.0, false)
        } else if Self::percent_is_trustworthy(value) {
            (value, true)
        } else {
            (value.clamp(0.0, 100.0), false)
        }
    }

    /// Real UTC Gregorian month length ending at `resets_at`, in minutes.
    ///
    /// Mirrors upstream `ProviderPaceCapability.inferredMonthlyWindowMinutes`
    /// (reset − 1 calendar month). Used so monthly pace scores the actual cycle
    /// (28–31 days) instead of a flat 30-day sentinel.
    pub fn calendar_month_window_minutes(resets_at: DateTime<Utc>) -> Option<u32> {
        let start = subtract_one_calendar_month(resets_at)?;
        let minutes = (resets_at - start).num_minutes();
        if minutes > 0 {
            Some(minutes as u32)
        } else {
            None
        }
    }

    /// Monthly window minutes from a known reset. Returns `None` when there is
    /// no reset (upstream leaves windowMinutes unset in that case).
    pub fn monthly_window_minutes(resets_at: Option<DateTime<Utc>>) -> Option<u32> {
        resets_at.and_then(Self::calendar_month_window_minutes)
    }

    /// Get the remaining percentage (100 - used)
    pub fn remaining_percent(&self) -> f64 {
        100.0 - self.used_percent
    }

    /// Check if the window is exhausted (>= 100% used)
    pub fn is_exhausted(&self) -> bool {
        self.used_percent >= 100.0
    }

    /// Check if the window is nearly exhausted (>= 90% used)
    pub fn is_nearly_exhausted(&self) -> bool {
        self.used_percent >= 90.0
    }

    /// Format the reset time as a countdown string
    pub fn format_countdown(&self) -> Option<String> {
        let resets_at = self.resets_at?;
        let now = Utc::now();

        if resets_at <= now {
            return Some("now".to_string());
        }

        let duration = resets_at - now;
        let hours = duration.num_hours();
        let total_minutes = ((duration.num_seconds() + 59) / 60).max(1);
        let minutes = total_minutes % 60;

        if hours > 24 {
            let days = hours / 24;
            Some(format!("{}d {}h", days, hours % 24))
        } else if hours > 0 {
            Some(format!("{}h {}m", hours, minutes))
        } else {
            Some(format!("{}m", minutes))
        }
    }
}

/// Subtract one Gregorian calendar month in UTC (upstream Calendar.date(byAdding: .month, -1)).
fn subtract_one_calendar_month(dt: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let y = dt.year();
    let m = dt.month();
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    let max_day = days_in_month(py, pm);
    let day = dt.day().min(max_day);
    dt.date_naive()
        .with_year(py)?
        .with_month(pm)?
        .with_day(day)
        .map(|d| d.and_time(dt.time()).and_utc())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some() {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

impl Default for RateWindow {
    /// A defaulted window carries no upstream reading, so its `0.0` is a
    /// placeholder rather than a measurement — it is never authoritative.
    fn default() -> Self {
        Self::new(0.0).non_authoritative()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn no_active_session_is_informational_five_hour_placeholder() {
        let window = RateWindow::no_active_session();

        assert!(window.is_informational);
        assert_eq!(window.window_minutes, Some(SESSION_WINDOW_MINUTES));
        assert_eq!(
            window.reset_description.as_deref(),
            Some("No active 5h session")
        );
        assert_eq!(window.used_percent, 0.0);
        assert_eq!(window.resets_at, None);
    }

    #[test]
    fn test_remaining_percent() {
        let window = RateWindow::new(75.0);
        assert!((window.remaining_percent() - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clamping() {
        let window = RateWindow::new(150.0);
        assert!((window.used_percent - 100.0).abs() < f64::EPSILON);

        let window = RateWindow::new(-10.0);
        assert!(window.used_percent.abs() < f64::EPSILON);
    }

    #[test]
    fn test_exhausted() {
        assert!(RateWindow::new(100.0).is_exhausted());
        assert!(!RateWindow::new(99.0).is_exhausted());
    }

    #[test]
    fn countdown_uses_one_minute_for_sub_minute_future_reset() {
        let window = RateWindow::with_details(
            10.0,
            None,
            Some(Utc::now() + chrono::Duration::seconds(30)),
            None,
        );

        assert_eq!(window.format_countdown().as_deref(), Some("1m"));
    }

    #[test]
    fn calendar_month_window_uses_real_cycle_length() {
        // March 1 2026 ends a 28-day February cycle (upstream ProviderPaceCapabilityTests).
        let resets = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        assert_eq!(
            RateWindow::calendar_month_window_minutes(resets),
            Some(28 * 24 * 60)
        );
        // 31-day cycle ending Aug 1.
        let resets = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
        assert_eq!(
            RateWindow::calendar_month_window_minutes(resets),
            Some(31 * 24 * 60)
        );
        assert_eq!(RateWindow::monthly_window_minutes(None), None);
    }

    #[test]
    fn cadence_boundary_session_exactly_300() {
        assert_eq!(
            RateWindowCadence::from_minutes(300),
            RateWindowCadence::Session
        );
    }

    #[test]
    fn cadence_boundary_weekly_10080() {
        assert_eq!(
            RateWindowCadence::from_minutes(10_080),
            RateWindowCadence::Weekly
        );
    }

    #[test]
    fn cadence_boundary_below_monthly_43199_is_weekly() {
        assert_eq!(
            RateWindowCadence::from_minutes(43_199),
            RateWindowCadence::Weekly
        );
    }

    #[test]
    fn cadence_boundary_monthly_43200() {
        assert_eq!(
            RateWindowCadence::from_minutes(43_200),
            RateWindowCadence::Monthly
        );
    }

    #[test]
    fn cadence_from_seconds_rounding() {
        // from_seconds rounds up: (seconds + 59) / 60
        // 300 min = 18000s exactly → Session
        assert_eq!(
            RateWindowCadence::from_seconds(18_000),
            RateWindowCadence::Session
        );
        // 18001s → (18001+59)/60 = 301 min → Unknown (not exactly 300)
        assert_eq!(
            RateWindowCadence::from_seconds(18_001),
            RateWindowCadence::Unknown
        );
        // 10080 min = 604800s → Weekly
        assert_eq!(
            RateWindowCadence::from_seconds(604_800),
            RateWindowCadence::Weekly
        );
        // 43200 min = 2592000s → Monthly
        assert_eq!(
            RateWindowCadence::from_seconds(2_592_000),
            RateWindowCadence::Monthly
        );
        // 0 or negative → Unknown
        assert_eq!(
            RateWindowCadence::from_seconds(0),
            RateWindowCadence::Unknown
        );
        assert_eq!(
            RateWindowCadence::from_seconds(-1),
            RateWindowCadence::Unknown
        );
    }

    #[test]
    fn cadence_label_keys() {
        assert_eq!(RateWindowCadence::Session.label_key(), "session");
        assert_eq!(RateWindowCadence::Weekly.label_key(), "weekly");
        assert_eq!(RateWindowCadence::Monthly.label_key(), "monthly");
        assert_eq!(RateWindowCadence::Unknown.label_key(), "unknown");
    }

    // ---------------------------------------------------------------------
    // Quota-authority hardening.
    //
    // Invariant under test: invalid or unknown authority moves toward
    // unavailable / `None`, never toward a more favorable remaining percentage.
    // ---------------------------------------------------------------------

    fn session(used_percent: f64) -> RateWindow {
        RateWindow::with_details(used_percent, Some(SESSION_WINDOW_MINUTES), None, None)
    }

    // --- A. valid numeric authority -------------------------------------

    #[test]
    fn authority_zero_percent_stays_authoritative_zero() {
        let window = session(0.0);
        assert!(window.quota_authoritative);
        assert_eq!(window.used_percent, 0.0);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            Some(0.0)
        );
    }

    #[test]
    fn authority_hundred_percent_stays_authoritative_hundred() {
        let window = session(100.0);
        assert!(window.quota_authoritative);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            Some(100.0)
        );
    }

    #[test]
    fn authority_preserves_valid_percents_bit_for_bit() {
        for value in [37.5_f64, 0.4_f64] {
            let window = session(value);
            assert_eq!(
                window.used_percent, value,
                "{value} must not be rounded or truncated"
            );
            assert_eq!(
                window.authoritative_used_percent(RateWindowCadence::Session),
                Some(value)
            );
        }
    }

    #[test]
    fn authority_requires_the_expected_cadence() {
        let window = session(37.5);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Weekly),
            None,
            "a 300-minute window is never weekly authority"
        );
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Monthly),
            None
        );
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Unknown),
            None
        );
    }

    // --- B. invalid numeric authority -----------------------------------

    #[test]
    fn nan_never_becomes_authoritative_zero() {
        let window = session(f64::NAN);
        assert!(
            window.used_percent.is_finite(),
            "NaN must not be retained: it serializes to JSON null"
        );
        assert_eq!(window.used_percent, 0.0);
        assert!(!window.quota_authoritative);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            None
        );
        assert_eq!(window.trusted_used_percent(), None);
    }

    #[test]
    fn positive_infinity_never_becomes_authoritative_zero() {
        let window = session(f64::INFINITY);
        assert!(window.used_percent.is_finite());
        assert_eq!(window.used_percent, 0.0);
        assert!(!window.quota_authoritative);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            None
        );
    }

    #[test]
    fn negative_infinity_never_becomes_authoritative_zero() {
        let window = session(f64::NEG_INFINITY);
        assert!(window.used_percent.is_finite());
        assert_eq!(window.used_percent, 0.0);
        assert!(!window.quota_authoritative);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            None
        );
    }

    #[test]
    fn out_of_range_percents_are_unavailable_even_after_clamping() {
        // Historical clamp is preserved for display; authority is not.
        let low = session(-0.1);
        assert_eq!(low.used_percent, 0.0);
        assert!(!low.quota_authoritative);
        assert_eq!(
            low.authoritative_used_percent(RateWindowCadence::Session),
            None
        );

        let high = session(100.1);
        assert_eq!(high.used_percent, 100.0);
        assert!(!high.quota_authoritative);
        assert_eq!(
            high.authoritative_used_percent(RateWindowCadence::Session),
            None
        );
    }

    #[test]
    fn negative_percent_cannot_report_full_remaining_headroom() {
        // The -50% → clamp(0) → "100% remaining" laundering path.
        let window = session(-50.0);
        assert_eq!(window.remaining_percent(), 100.0);
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            None,
            "a clamped negative percent must not read as a fully-available quota"
        );
    }

    #[test]
    fn authority_cannot_be_regranted_after_construction_denied_it() {
        // The NaN placeholder stores a perfectly valid-looking 0.0, so the
        // builder must not be able to revalidate its way back to authority.
        let window = session(f64::NAN).with_quota_authority(true);
        assert!(
            !window.quota_authoritative,
            "with_quota_authority may only narrow authority, never launder it"
        );
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            None
        );

        let window = session(150.0).with_quota_authority(true);
        assert!(!window.quota_authoritative);

        let window = RateWindow::informational("note").with_quota_authority(true);
        assert!(!window.quota_authoritative);
    }

    // --- C. missing authority (shared shapes) ---------------------------

    #[test]
    fn informational_rows_are_never_authoritative() {
        let informational = RateWindow::informational("¥12.00");
        assert!(!informational.quota_authoritative);
        assert_eq!(informational.trusted_used_percent(), None);

        let placeholder = RateWindow::no_active_session();
        assert!(!placeholder.quota_authoritative);
        assert_eq!(
            placeholder.authoritative_used_percent(RateWindowCadence::Session),
            None
        );
    }

    #[test]
    fn defaulted_window_is_not_authoritative() {
        let window = RateWindow::default();
        assert_eq!(window.used_percent, 0.0);
        assert!(!window.quota_authoritative);
        assert_eq!(window.trusted_used_percent(), None);
    }

    #[test]
    fn missing_window_minutes_has_no_cadence_authority() {
        let window = RateWindow::new(20.0);
        assert!(window.quota_authoritative);
        assert_eq!(window.trusted_used_percent(), Some(20.0));
        assert_eq!(
            window.authoritative_used_percent(RateWindowCadence::Session),
            None,
            "cadence authority requires a declared window_minutes"
        );
    }

    // --- D. cadence ------------------------------------------------------

    #[test]
    fn cadence_authority_matrix() {
        let cases = [
            (300_u32, RateWindowCadence::Session, true),
            (300, RateWindowCadence::Weekly, false),
            (10_080, RateWindowCadence::Weekly, true),
            (20_160, RateWindowCadence::Weekly, true),
            (43_199, RateWindowCadence::Weekly, true),
            (43_200, RateWindowCadence::Weekly, false),
            (43_200, RateWindowCadence::Monthly, true),
        ];

        for (minutes, expected, authoritative) in cases {
            let window = RateWindow::with_details(42.0, Some(minutes), None, None);
            assert_eq!(
                window.authoritative_used_percent(expected).is_some(),
                authoritative,
                "{minutes} minutes vs {expected:?}"
            );
        }
    }

    // --- E. serialization / authority boundary ---------------------------

    #[test]
    fn authority_does_not_survive_a_json_round_trip() {
        let original = session(37.5);
        assert!(original.quota_authoritative);

        let json = serde_json::to_string(&original).expect("serialize");
        assert!(
            !json.contains("quota_authoritative"),
            "authority must stay out of the public usage JSON: {json}"
        );

        let restored: RateWindow = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            restored.used_percent, 37.5,
            "the public value round-trips unchanged"
        );
        assert!(
            !restored.quota_authoritative,
            "deserialization must fail closed — authority is never inferred from bytes"
        );
        assert_eq!(
            restored.authoritative_used_percent(RateWindowCadence::Session),
            None
        );
        assert_eq!(restored.trusted_used_percent(), None);
    }

    #[test]
    fn public_usage_json_schema_is_unchanged() {
        let window = RateWindow::with_details(
            37.5,
            Some(300),
            Some(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()),
            Some("Mar 1".to_string()),
        );
        let value = serde_json::to_value(&window).expect("serialize");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "is_informational",
                "reset_description",
                "resets_at",
                "used_percent",
                "window_minutes",
            ]
        );

        // Optional fields still collapse out of the payload when absent.
        let sparse = serde_json::to_value(RateWindow::new(1.0)).expect("serialize");
        let mut sparse_keys: Vec<&str> = sparse
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        sparse_keys.sort_unstable();
        assert_eq!(sparse_keys, ["is_informational", "used_percent"]);
    }

    #[test]
    fn non_finite_percent_is_never_serialized_as_null() {
        // serde_json renders NaN/±Infinity as `null`, which would break the
        // `f64` contract for every downstream consumer.
        let json = serde_json::to_value(session(f64::NAN)).expect("serialize");
        assert_eq!(json["used_percent"], serde_json::json!(0.0));
        assert!(json["used_percent"].is_number());
    }
}
