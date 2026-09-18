//! Parser for the structured `agy -p /usage --output-format json` report.
//!
//! The signed-in `agy` CLI answers `/usage` with a quota envelope that groups
//! buckets by model family (`Gemini Models`, `Claude and GPT models`), each
//! group carrying a 5-hour and a weekly bucket. This module turns that envelope
//! into a [`UsageSnapshot`].
//!
//! # Quota authority
//!
//! A bucket's `remaining_fraction` becomes an authoritative percentage only
//! when the raw upstream value is present, finite, and inside `[0.0, 1.0]`.
//! Everything else — missing, disabled, non-finite, out of range — is *absent
//! evidence*, and absent evidence must move a lane toward unavailable, never
//! toward a favorable "0% used / 100% remaining" reading. In particular the
//! fraction is never clamped before authority is decided: a clamped number is a
//! fabrication, not a measurement. See [`authoritative_used_percent`].

use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::core::{
    NamedRateWindow, ProviderError, RateWindow, SESSION_WINDOW_MINUTES, UsageSnapshot,
    WEEKLY_WINDOW_MINUTES,
};

const WINDOW_ID_PREFIX: &str = "antigravity-quota-summary-";

/// Group rank for the model family that owns the summary lanes.
const GEMINI_GROUP_RANK: u8 = 0;

/// Top-level structured CLI envelope.
///
/// `status` and `command.name` are required and validated; any other field the
/// CLI adds is ignored.
#[derive(Debug, Deserialize)]
struct CliUsageReport {
    status: String,
    command: CliUsageCommand,
}

#[derive(Debug, Deserialize)]
struct CliUsageCommand {
    name: String,
    data: QuotaSummaryPayload,
}

/// `command.data`. `groups` is mandatory: a payload without it carries no
/// quota evidence at all and is rejected rather than treated as empty.
#[derive(Debug, Deserialize)]
struct QuotaSummaryPayload {
    groups: Vec<QuotaSummaryGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryGroup {
    display_name: Option<String>,
    name: Option<String>,
    /// A group with no buckets contributes no lanes; it is not an error on its
    /// own, because another group may still carry usable quota.
    #[serde(default)]
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryBucket {
    bucket_id: Option<String>,
    id: Option<String>,
    display_name: Option<String>,
    name: Option<String>,
    /// Explicit cadence label (`"5h"`, `"weekly"`), preferred over the id and
    /// display name so lane identity never depends on array order.
    window: Option<String>,
    disabled: Option<bool>,
    #[serde(alias = "remaining_fraction")]
    remaining_fraction: Option<f64>,
    remaining: Option<QuotaSummaryRemaining>,
    #[serde(alias = "reset_time")]
    reset_time: Option<String>,
}

/// Nested `remaining` oneof emitted by some CLI builds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryRemaining {
    #[serde(alias = "remaining_fraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "case")]
    oneof_case: Option<String>,
    value: Option<f64>,
}

impl QuotaSummaryBucket {
    /// The raw upstream remaining fraction, unsanitized.
    ///
    /// This deliberately performs no range or finiteness checks — deciding
    /// authority is [`authoritative_used_percent`]'s job, and it must see the
    /// raw value.
    fn raw_remaining_fraction(&self) -> Option<f64> {
        self.remaining_fraction.or_else(|| {
            let remaining = self.remaining.as_ref()?;
            remaining.remaining_fraction.or_else(|| {
                (remaining.oneof_case.as_deref() == Some("remainingFraction"))
                    .then_some(remaining.value)
                    .flatten()
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BucketKind {
    Session,
    Weekly,
    Other,
}

/// Parse a structured `agy /usage` report into a usage snapshot.
pub(super) fn parse_cli_usage_report(data: &[u8]) -> Result<UsageSnapshot, ProviderError> {
    let report: CliUsageReport = serde_json::from_slice(data)
        .map_err(|error| ProviderError::Parse(format!("Antigravity CLI usage report: {error}")))?;
    if report.status != "SUCCESS" || report.command.name != "usage" {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage report was not successful".into(),
        ));
    }
    build_usage_snapshot(report.command.data.groups)
}

/// Derive an authoritative used-percent from a raw remaining fraction.
///
/// Returns `Some` only when the raw evidence is present, finite, and within
/// `[0.0, 1.0]`. Out-of-range and non-finite fractions return `None` rather
/// than being clamped into a plausible-looking percentage, so a malformed
/// reading can never be laundered into "0% used".
fn authoritative_used_percent(remaining_fraction: Option<f64>) -> Option<f64> {
    let fraction = remaining_fraction?;
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    // `fraction ∈ [0, 1]` ⇒ `fraction * 100 ∈ [0, 100]` ⇒ result ∈ [0, 100].
    Some(100.0 - fraction * 100.0)
}

fn build_usage_snapshot(groups: Vec<QuotaSummaryGroup>) -> Result<UsageSnapshot, ProviderError> {
    let ranked = ranked_quota_windows(groups);

    if !ranked.iter().any(|(_, window)| window.usage_known) {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage report has no usable quota buckets".into(),
        ));
    }

    // The Gemini group owns the summary lanes when it is present; a third-party
    // group must never stand in for a Gemini cadence that the report omitted.
    let has_gemini_group = ranked.iter().any(|(rank, _)| *rank == GEMINI_GROUP_RANK);
    let candidates: Vec<&NamedRateWindow> = ranked
        .iter()
        .filter(|(rank, _)| !has_gemini_group || *rank == GEMINI_GROUP_RANK)
        .map(|(_, window)| window)
        .collect();

    let primary = most_constrained(&candidates, SESSION_WINDOW_MINUTES);
    let secondary = most_constrained(&candidates, WEEKLY_WINDOW_MINUTES);

    // An absent cadence stays absent: the placeholder is informational and
    // therefore non-authoritative, never a fabricated 0%-used lane.
    let mut snapshot = match primary {
        Some(window) => UsageSnapshot::new(window.window.clone()),
        None => UsageSnapshot::new(RateWindow::no_active_session()),
    };
    if let Some(window) = secondary {
        snapshot = snapshot.with_secondary(window.window.clone());
    }

    // Every bucket stays addressable as a labeled lane (matching the local
    // probe's convention), so the Claude/GPT windows remain visible instead of
    // displacing the Gemini summary lanes.
    snapshot.extra_rate_windows = ranked.into_iter().map(|(_, window)| window).collect();

    Ok(snapshot)
}

/// Build every group's lanes, tagged with the group's family rank.
///
/// Groups are visited in document order so window-id scoping is deterministic;
/// selection later keys off the rank, not the position.
fn ranked_quota_windows(groups: Vec<QuotaSummaryGroup>) -> Vec<(u8, NamedRateWindow)> {
    let mut used_window_ids = HashSet::new();
    let mut scope_counts: HashMap<String, usize> = HashMap::new();
    let mut ranked = Vec::new();

    for group in &groups {
        let rank = group_rank(group);
        let base_scope = group_scope(group);
        let occurrence = scope_counts.entry(base_scope.clone()).or_default();
        let scoped_group = if *occurrence == 0 {
            base_scope
        } else {
            format!("{base_scope}-{occurrence}")
        };
        *occurrence += 1;
        for window in group_quota_windows(group, &scoped_group, &mut used_window_ids) {
            ranked.push((rank, window));
        }
    }

    ranked
}

fn group_quota_windows(
    group: &QuotaSummaryGroup,
    group_scope: &str,
    used_window_ids: &mut HashSet<String>,
) -> Vec<NamedRateWindow> {
    let group_title = group_title(group);
    let mut buckets = group.buckets.iter().enumerate().collect::<Vec<_>>();
    buckets.sort_by_key(|(index, bucket)| (bucket_kind(bucket), *index));

    let mut windows = Vec::new();
    for (_, bucket) in buckets {
        let Some(bucket_id) = non_empty(bucket.bucket_id.as_deref().or(bucket.id.as_deref()))
        else {
            continue;
        };
        let kind = bucket_kind(bucket);
        let title = format!("{group_title} {}", bucket_title(bucket, kind));

        // A disabled bucket reports a stale number, so it carries no authority
        // even when its fraction parses cleanly.
        let enabled = !bucket.disabled.unwrap_or(false);
        let used_percent = enabled
            .then(|| authoritative_used_percent(bucket.raw_remaining_fraction()))
            .flatten();

        let window_minutes = match kind {
            BucketKind::Session => Some(SESSION_WINDOW_MINUTES),
            BucketKind::Weekly => Some(WEEKLY_WINDOW_MINUTES),
            BucketKind::Other => None,
        };
        let resets_at = bucket.reset_time.as_deref().and_then(parse_reset_time);

        // `unwrap_or(0.0)` is a display placeholder only; the paired
        // `with_quota_authority` call keeps it out of every quota decision.
        let window =
            RateWindow::with_details(used_percent.unwrap_or(0.0), window_minutes, resets_at, None)
                .with_quota_authority(used_percent.is_some());

        let base_id = format!("{WINDOW_ID_PREFIX}{group_scope}-{bucket_id}");
        let id = unique_window_id(base_id, group_scope, used_window_ids);
        windows
            .push(NamedRateWindow::new(id, title, window).with_usage_known(used_percent.is_some()));
    }
    windows
}

fn unique_window_id(
    base_id: String,
    group_scope: &str,
    used_window_ids: &mut HashSet<String>,
) -> String {
    if used_window_ids.insert(base_id.clone()) {
        return base_id;
    }

    let mut ordinal = 1;
    loop {
        let candidate = format!("{base_id}-{group_scope}-{ordinal}");
        if used_window_ids.insert(candidate.clone()) {
            return candidate;
        }
        ordinal += 1;
    }
}

/// The most-used authoritative lane at the requested cadence.
///
/// Only lanes whose percentage survives [`RateWindow::trusted_used_percent`]
/// are eligible, so a promoted summary lane is always backed by real evidence.
fn most_constrained<'a>(
    windows: &[&'a NamedRateWindow],
    minutes: u32,
) -> Option<&'a NamedRateWindow> {
    windows
        .iter()
        .copied()
        .filter(|row| row.usage_known && row.window.window_minutes == Some(minutes))
        .filter_map(|row| row.window.trusted_used_percent().map(|used| (used, row)))
        .max_by(|(left_used, left), (right_used, right)| {
            left_used
                .partial_cmp(right_used)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.id.cmp(&left.id))
        })
        .map(|(_, row)| row)
}

fn group_rank(group: &QuotaSummaryGroup) -> u8 {
    let title = raw_group_title(group).to_ascii_lowercase();
    if title.contains("gemini") {
        GEMINI_GROUP_RANK
    } else if title.contains("claude") || title.contains("gpt") {
        1
    } else {
        2
    }
}

fn raw_group_title(group: &QuotaSummaryGroup) -> &str {
    non_empty(group.display_name.as_deref().or(group.name.as_deref())).unwrap_or("Quota")
}

/// Display prefix for a group's lanes.
///
/// `Gemini Models` shortens to `Gemini` so the summary lanes read
/// `Gemini 5h` / `Gemini Weekly`; every other group keeps the name the CLI
/// reported (`Claude and GPT models 5h`).
fn group_title(group: &QuotaSummaryGroup) -> String {
    let title = raw_group_title(group);
    if title.to_ascii_lowercase().contains("gemini") {
        "Gemini".to_string()
    } else {
        title.to_string()
    }
}

fn group_scope(group: &QuotaSummaryGroup) -> String {
    group_title(group)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Classify a bucket's cadence from its explicit labels, never from position.
fn bucket_kind(bucket: &QuotaSummaryBucket) -> BucketKind {
    [
        bucket.window.as_deref(),
        bucket.bucket_id.as_deref().or(bucket.id.as_deref()),
        bucket.display_name.as_deref().or(bucket.name.as_deref()),
    ]
    .into_iter()
    .flatten()
    .find_map(classify_cadence_token)
    .unwrap_or(BucketKind::Other)
}

const SESSION_ALIASES: [&str; 5] = ["session", "5h", "5-hour", "five hour", "five-hour"];

/// Match one label against the known cadence vocabulary.
///
/// Trailing ` limit` / ` remaining` decorations are stripped so
/// `"Five Hour Limit Remaining"` and `"5h"` classify identically.
fn classify_cadence_token(raw: &str) -> Option<BucketKind> {
    let normalized = raw.trim().to_ascii_lowercase().replace('_', "-");
    let mut candidate = normalized.trim();
    while let Some(stripped) = candidate
        .strip_suffix(" remaining")
        .or_else(|| candidate.strip_suffix(" limit"))
    {
        candidate = stripped.trim_end();
    }
    if candidate.is_empty() {
        return None;
    }

    if SESSION_ALIASES
        .iter()
        .any(|alias| candidate == *alias || candidate.ends_with(&format!("-{alias}")))
    {
        Some(BucketKind::Session)
    } else if candidate == "weekly" || candidate.ends_with("-weekly") {
        Some(BucketKind::Weekly)
    } else {
        None
    }
}

fn bucket_title(bucket: &QuotaSummaryBucket, kind: BucketKind) -> String {
    match kind {
        BucketKind::Session => "5h".into(),
        BucketKind::Weekly => "Weekly".into(),
        BucketKind::Other => non_empty(bucket.display_name.as_deref().or(bucket.name.as_deref()))
            .or_else(|| non_empty(bucket.bucket_id.as_deref().or(bucket.id.as_deref())))
            .unwrap_or("quota")
            .to_string(),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parse_reset_time(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
        .or_else(|| {
            let seconds = raw.parse::<f64>().ok()?;
            if !seconds.is_finite() {
                return None;
            }
            let whole = seconds.trunc();
            if whole < i64::MIN as f64 || whole > i64::MAX as f64 {
                return None;
            }
            Utc.timestamp_opt(whole as i64, 0).single()
        })
}

#[cfg(test)]
#[path = "quota_summary_tests.rs"]
mod tests;
