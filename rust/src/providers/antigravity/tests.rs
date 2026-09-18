use super::*;
use crate::core::RateWindowCadence;

#[test]
fn test_classify_model_families() {
    assert_eq!(classify_model("Claude 3.5 Sonnet"), ModelFamily::Claude);
    assert_eq!(classify_model("claude-4-opus"), ModelFamily::Claude);
    assert_eq!(
        classify_model("Claude Thinking"),
        ModelFamily::ClaudeThinking
    );
    assert_eq!(
        classify_model("claude-3.5-sonnet-thinking"),
        ModelFamily::ClaudeThinking
    );
    assert_eq!(classify_model("Gemini 2.5 Pro Low"), ModelFamily::GeminiPro);
    assert_eq!(classify_model("gemini-pro-low"), ModelFamily::GeminiPro);
    assert_eq!(classify_model("Pro Low Latency"), ModelFamily::GeminiPro);
    assert_eq!(classify_model("Gemini 2.5 Flash"), ModelFamily::GeminiFlash);
    assert_eq!(classify_model("gemini-flash"), ModelFamily::GeminiFlash);
    assert_eq!(classify_model("Flash Model"), ModelFamily::GeminiFlash);
    assert_eq!(classify_model("GPT-4o"), ModelFamily::Other);
    assert_eq!(classify_model("unknown-model"), ModelFamily::Other);
}

#[test]
fn retired_flash_ids_collapse_to_current_wire_id() {
    for id in [
        "gemini-3.6-flash",
        "gemini-3.6-flash-high",
        "gemini-3.5-flash-extra-low",
        "gemini-3-flash-agent",
    ] {
        assert_eq!(canonical_model_id(id), "gemini-3.7-flash");
    }
    assert_eq!(canonical_model_id("gemini-3.7-flash"), "gemini-3.7-flash");
}

#[test]
fn parses_current_language_server_process() {
    let output = r"4242	C:\Users\test\AppData\Local\Programs\Antigravity\resources\bin\language_server.exe --csrf_token 11111111-2222-3333-4444-555555555555 --extension_server_port 54123";

    let process = AntigravityProvider::parse_process_info(output).expect("process info");

    assert_eq!(process.pid, Some(4242));
    assert_eq!(process.extension_port, Some(54123));
    assert_eq!(process.csrf_token, "11111111-2222-3333-4444-555555555555");
    assert_eq!(process.source, ProcessSource::Ide);
}

#[test]
fn parses_language_server_without_extension_server_port() {
    let output = "34564\tC:\\Users\\test\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server.exe --standalone --override_ide_name antigravity --subclient_type hub --override_ide_version 2.0.11 --https_server_port 0 --csrf_token 68dda2fb-6b26-40c0-aeef-b9a628615714 --app_data_dir antigravity";

    let process =
        AntigravityProvider::parse_process_info(output).expect("process info should be detected");

    assert_eq!(process.pid, Some(34564));
    assert_eq!(process.extension_port, Some(0));
    assert_eq!(process.csrf_token, "68dda2fb-6b26-40c0-aeef-b9a628615714");
    assert_eq!(process.source, ProcessSource::Ide);
}

#[test]
fn parses_language_server_without_any_port_arg() {
    let output = "34564\tC:\\Users\\test\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server.exe --standalone --csrf_token aabbccdd-1122-3344-5566-778899001122 --app_data_dir antigravity";

    let process =
        AntigravityProvider::parse_process_info(output).expect("process info should be detected");

    assert_eq!(process.pid, Some(34564));
    assert_eq!(process.extension_port, None);
    assert_eq!(process.csrf_token, "aabbccdd-1122-3344-5566-778899001122");
    assert_eq!(process.source, ProcessSource::Ide);
}

#[test]
fn parses_equals_form_args() {
    let output = "34564\tC:\\Users\\test\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server.exe --csrf_token=68dda2fb-6b26-40c0-aeef-b9a628615714 --https_server_port=61999";

    let process =
        AntigravityProvider::parse_process_info(output).expect("process info should be detected");

    assert_eq!(process.pid, Some(34564));
    assert_eq!(process.extension_port, Some(61999));
    assert_eq!(process.csrf_token, "68dda2fb-6b26-40c0-aeef-b9a628615714");
    assert_eq!(process.source, ProcessSource::Ide);
}

fn make_response(models: Vec<(&str, f64)>) -> UserStatusResponse {
    let json = serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": models.iter().map(|(label, remaining)| {
                    serde_json::json!({
                        "label": label,
                        "quotaInfo": {
                            "remainingFraction": remaining
                        }
                    })
                }).collect::<Vec<_>>()
            }
        }
    });
    serde_json::from_value(json).unwrap()
}

#[test]
fn antigravity_extra_windows_preserve_usage_known() {
    let json = serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    {
                        "label": "Gemini 2.5 Pro",
                        "quotaInfo": {"remainingFraction": 0.8}
                    },
                    {
                        "label": "Claude 4 Sonnet",
                        "quotaInfo": {"remainingFraction": null}
                    }
                ]
            }
        }
    });
    let resp: UserStatusResponse = serde_json::from_value(json).unwrap();
    let snap = AntigravityProvider::new().parse_user_status(resp).unwrap();
    let gemini = snap
        .extra_rate_windows
        .iter()
        .find(|window| window.title.contains("Gemini"))
        .unwrap();
    let claude = snap
        .extra_rate_windows
        .iter()
        .find(|window| window.title.contains("Claude"))
        .unwrap();
    assert!(gemini.usage_known);
    assert!(!claude.usage_known);
    assert_eq!(claude.window.used_percent, 0.0);
}

#[test]
fn test_parse_user_status_standard() {
    let resp = make_response(vec![
        ("Claude 3.5 Sonnet", 0.8),
        ("Gemini 2.5 Pro Low", 0.5),
        ("Gemini 2.5 Flash", 0.9),
    ]);
    let provider = AntigravityProvider::new();
    let snap = provider.parse_user_status(resp).unwrap();

    assert!((snap.primary.used_percent - 20.0).abs() < 0.1);
    let sec = snap.secondary.unwrap();
    assert!((sec.used_percent - 50.0).abs() < 0.1);
    let ter = snap.model_specific.unwrap();
    assert!((ter.used_percent - 10.0).abs() < 0.1);
    assert_eq!(snap.extra_rate_windows.len(), 3);
    assert!(
        snap.extra_rate_windows
            .iter()
            .any(|window| window.title == "Gemini 2.5 Flash")
    );
}

#[test]
fn test_parse_user_status_thinking_skipped() {
    let resp = make_response(vec![
        ("Claude Thinking", 0.6),
        ("Claude 3.5 Sonnet", 0.7),
        ("Gemini 2.5 Flash", 0.5),
    ]);
    let provider = AntigravityProvider::new();
    let snap = provider.parse_user_status(resp).unwrap();

    assert!((snap.primary.used_percent - 30.0).abs() < 0.1);
}

#[test]
fn test_parse_user_status_fallback_first() {
    let resp = make_response(vec![("GPT-4o", 0.4), ("Mistral Large", 0.6)]);
    let provider = AntigravityProvider::new();
    let snap = provider.parse_user_status(resp).unwrap();

    assert!((snap.primary.used_percent - 60.0).abs() < 0.1);
    assert!(snap.secondary.is_none());
    assert!(snap.model_specific.is_none());
}

#[test]
fn test_noisy_models_do_not_drive_summary_windows() {
    let resp = make_response(vec![
        ("Gemini 2.5 Flash Image", 0.01),
        ("Gemini 2.5 Pro Lite", 0.02),
        ("Gemini autocomplete internal", 0.03),
        ("Claude 4 Sonnet", 0.8),
        ("Gemini 2.5 Pro Low", 0.6),
        ("Gemini 2.5 Flash", 0.7),
    ]);
    let provider = AntigravityProvider::new();
    let snap = provider.parse_user_status(resp).unwrap();

    assert!((snap.primary.used_percent - 20.0).abs() < 0.1);
    assert!((snap.secondary.unwrap().used_percent - 40.0).abs() < 0.1);
    assert!((snap.model_specific.unwrap().used_percent - 30.0).abs() < 0.1);
    assert!(
        snap.extra_rate_windows
            .iter()
            .any(|window| window.title == "Gemini 2.5 Flash Image")
    );
}

#[test]
fn not_running_error_tells_user_how_to_start() {
    let error = ProviderError::NotInstalled(NOT_RUNNING_MESSAGE.to_string()).to_string();

    assert!(error.contains("Start Google Antigravity and sign in"));
}

// ── agy CLI process matching ───────────────────────────────────────

#[test]
fn detects_agy_exe_cli_process_with_empty_csrf() {
    // agy.exe hosts the language server in-process with no --csrf_token.
    let output =
        "7777\tC:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe session --model gemini-2.5-pro";

    let process = AntigravityProvider::parse_process_info(output)
        .expect("agy CLI process should be detected");

    assert_eq!(process.pid, Some(7777));
    assert_eq!(process.source, ProcessSource::Cli);
    assert_eq!(process.csrf_token, "");
    assert!(
        process.csrf_token.is_empty(),
        "agy CLI requires no CSRF token"
    );
    assert_eq!(process.extension_server_csrf_token, None);
    assert_eq!(process.extension_port, None);
}

#[test]
fn detects_quoted_agy_exe_cli_process() {
    // Windows CIM quotes an executable path that contains path separators.
    let output = "7777\t\"C:\\Users\\user\\AppData\\Local\\agy\\bin\\agy.exe\" --model gemini-3.7-flash-high";

    let process = AntigravityProvider::parse_process_info(output)
        .expect("quoted agy CLI process should be detected");

    assert_eq!(process.pid, Some(7777));
    assert_eq!(process.source, ProcessSource::Cli);
    assert!(process.csrf_token.is_empty());
}

#[test]
fn detects_bare_agy_command() {
    // The CLI may appear under the bare `agy` name (no .exe suffix).
    let output = "8888\tagy serve";

    let process = AntigravityProvider::parse_process_info(output)
        .expect("bare agy command should be detected");

    assert_eq!(process.pid, Some(8888));
    assert_eq!(process.source, ProcessSource::Cli);
    assert!(process.csrf_token.is_empty());
}

#[test]
fn detects_antigravity_cli_command() {
    // Upstream also matches antigravity-cli / antigravity_cli.
    let output = "9999\t/opt/homebrew/bin/antigravity-cli status";

    let process = AntigravityProvider::parse_process_info(output)
        .expect("antigravity-cli command should be detected");

    assert_eq!(process.pid, Some(9999));
    assert_eq!(process.source, ProcessSource::Cli);
    assert!(process.csrf_token.is_empty());
}

#[test]
fn ide_match_preferred_over_agy_cli_when_both_running() {
    // When the desktop IDE server and the agy CLI are both running, the
    // CSRF-protected IDE match wins (mirrors upstream process-kind precedence).
    let output = "4242\tC:\\Antigravity\\language_server.exe --csrf_token deadbeef-aaaa-bbbb-cccc-dddddddddddd --extension_server_port 54123\n\
                  7777\tC:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe session";

    let process =
        AntigravityProvider::parse_process_info(output).expect("a process should be detected");

    assert_eq!(process.pid, Some(4242));
    assert_eq!(process.source, ProcessSource::Ide);
    assert_eq!(process.csrf_token, "deadbeef-aaaa-bbbb-cccc-dddddddddddd");
}

#[test]
fn agy_cli_matches_when_only_cli_running() {
    // No --csrf_token anywhere: only the agy CLI line should match.
    let output = "7777\tC:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe";

    let process = AntigravityProvider::parse_process_info(output)
        .expect("agy CLI should be detected when it is the only match");

    assert_eq!(process.source, ProcessSource::Cli);
    assert!(process.csrf_token.is_empty());
}

#[test]
fn non_antigravity_process_without_csrf_is_not_matched() {
    // An unrelated tokenless process must not be mistaken for the agy CLI.
    let output = "1234\tC:\\Windows\\System32\\notepad.exe";

    let process = AntigravityProvider::parse_process_info(output);

    assert!(process.is_none(), "unrelated process must not match");
}

#[test]
fn is_agy_cli_command_matches_known_names() {
    assert!(is_agy_cli_command("agy serve"));
    assert!(is_agy_cli_command(
        "C:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe session"
    ));
    assert!(is_agy_cli_command(
        "C:\\Users\\user\\AppData\\Local\\agy\\bin\\AGY.EXE --model gemini-3.7-flash-high"
    ));
    assert!(is_agy_cli_command(
        "\"C:\\Users\\user\\AppData\\Local\\agy\\bin\\agy.exe\" --model gemini-3.7-flash-high"
    ));
    assert!(is_agy_cli_command("/usr/local/bin/antigravity-cli status"));
    assert!(is_agy_cli_command("/opt/antigravity_cli run"));
    assert!(is_agy_cli_command("\"C:\\Tools\\antigravity-cli\" status"));
    assert!(is_agy_cli_command("\"C:\\Tools\\antigravity_cli\" run"));
}

#[test]
fn is_agy_cli_command_rejects_unrelated_names() {
    // A leading path separator prevents `notantigravity-cli` from matching.
    assert!(!is_agy_cli_command(
        "notagy.exe --model gemini-3.7-flash-high"
    ));
    assert!(!is_agy_cli_command(
        "C:\\Tools\\someagy.exe --model gemini-3.7-flash-high"
    ));
    assert!(!is_agy_cli_command("notantigravity-cli status"));
    assert!(!is_agy_cli_command("C:\\Tools\\notantigravity-cli status"));
    assert!(!is_agy_cli_command("C:\\Windows\\System32\\notepad.exe"));
    assert!(!is_agy_cli_command("language_server.exe --csrf_token abc"));
    assert!(!is_agy_cli_command(""));
}

// ── Upstream 0.50.1 #2963: one lane per quota bucket ──────────────────────

#[test]
fn multiple_models_in_same_quota_bucket_collapse_to_one_lane() {
    // Two Claude variants sharing the same remaining fraction (same 5h
    // session bucket) should produce one extra rate window, not two.
    let resp = make_response(vec![
        ("Claude 3.5 Sonnet", 0.8),
        ("Claude 4 Sonnet", 0.8),
        ("Gemini 2.5 Pro Low", 0.5),
    ]);
    let provider = AntigravityProvider::new();
    let snap = provider.parse_user_status(resp).unwrap();
    assert_eq!(
        snap.extra_rate_windows.len(),
        2,
        "models sharing a quota bucket collapse to one lane"
    );
}

#[test]
fn models_in_distinct_quota_buckets_keep_separate_lanes() {
    let resp = make_response(vec![
        ("Claude 3.5 Sonnet", 0.8),
        ("Claude 4 Sonnet", 0.7),
        ("Gemini 2.5 Pro Low", 0.5),
    ]);
    let provider = AntigravityProvider::new();
    let snap = provider.parse_user_status(resp).unwrap();
    assert_eq!(snap.extra_rate_windows.len(), 3);
}

// ── Local-probe quota authority ───────────────────────────────────────────
//
// Invariant under test: missing, null, or invalid `remainingFraction` evidence
// must move a lane toward unavailable, never toward a favorable "0% used /
// 100% remaining" authoritative reading. An explicit raw `0.0` is a real
// measurement and must stay authoritative at 100% used.

/// Parse a single model config whose `quotaInfo` is the given JSON literal.
fn parse_single_quota(quota_info: serde_json::Value) -> UsageSnapshot {
    let json = serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    {"label": "Claude 4 Sonnet", "quotaInfo": quota_info}
                ]
            }
        }
    });
    let response: UserStatusResponse = serde_json::from_value(json).expect("fixture parses");
    AntigravityProvider::new()
        .parse_user_status(response)
        .expect("snapshot")
}

#[test]
fn local_remaining_one_is_trusted_zero_percent_used() {
    let snapshot = parse_single_quota(serde_json::json!({"remainingFraction": 1.0}));

    assert_eq!(snapshot.primary.trusted_used_percent(), Some(0.0));
    assert!(has_trusted_live_quota(&snapshot));
}

#[test]
fn local_explicit_zero_remaining_is_trusted_hundred_percent_used() {
    // The security-critical distinction: an explicit raw 0.0 is exhausted
    // quota, not missing evidence.
    let snapshot = parse_single_quota(serde_json::json!({"remainingFraction": 0.0}));

    assert_eq!(snapshot.primary.trusted_used_percent(), Some(100.0));
    assert!(snapshot.primary.quota_authoritative);
    assert!(has_trusted_live_quota(&snapshot));
    assert!(snapshot.extra_rate_windows[0].usage_known);
}

#[test]
fn local_fractional_remaining_is_trusted() {
    let snapshot = parse_single_quota(serde_json::json!({"remainingFraction": 0.31}));

    let used = snapshot.primary.trusted_used_percent().expect("trusted");
    assert!((used - 69.0).abs() < 0.001, "0.31 remaining → 69% used");
}

#[test]
fn local_missing_remaining_fraction_is_not_authoritative() {
    // `{"quotaInfo": {}}` deserializes cleanly because the field is optional.
    let snapshot = parse_single_quota(serde_json::json!({}));

    assert_eq!(snapshot.primary.trusted_used_percent(), None);
    assert!(!snapshot.primary.quota_authoritative);
    assert!(!has_trusted_live_quota(&snapshot));
}

#[test]
fn local_reset_time_without_remaining_fraction_is_not_authoritative() {
    let snapshot = parse_single_quota(serde_json::json!({"resetTime": "2026-01-01T00:00:00Z"}));

    assert_eq!(snapshot.primary.trusted_used_percent(), None);
    assert!(!has_trusted_live_quota(&snapshot));
}

#[test]
fn local_null_remaining_fraction_is_not_authoritative() {
    let snapshot = parse_single_quota(serde_json::json!({"remainingFraction": null}));

    assert_eq!(snapshot.primary.trusted_used_percent(), None);
    assert!(!has_trusted_live_quota(&snapshot));
    assert!(!snapshot.extra_rate_windows[0].usage_known);
}

#[test]
fn local_out_of_range_remaining_fraction_is_not_authoritative() {
    for fraction in [-0.1_f64, 1.1_f64, -1.0_f64, 2.0_f64] {
        let snapshot = parse_single_quota(serde_json::json!({"remainingFraction": fraction}));

        assert_eq!(
            snapshot.primary.trusted_used_percent(),
            None,
            "{fraction} is out of range and must not be laundered into authority"
        );
        assert!(!has_trusted_live_quota(&snapshot));
        assert!(!snapshot.extra_rate_windows[0].usage_known);
    }
}

#[test]
fn local_nonfinite_remaining_fraction_is_not_authoritative() {
    // JSON cannot carry NaN/±Infinity, but the struct can be built in-process,
    // so the authority gate must reject them at the source.
    for fraction in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let quota = QuotaInfo {
            remaining_fraction: Some(fraction),
            reset_time: None,
        };
        let window = rate_window_from_quota(&quota);

        assert!(
            !window.quota_authoritative,
            "{fraction} must not be trusted"
        );
        assert_eq!(window.trusted_used_percent(), None);
        assert!(
            window.used_percent.is_finite(),
            "a non-finite percent must never be stored"
        );
    }
}

#[test]
fn empty_client_model_configs_yields_no_authoritative_primary() {
    let json = serde_json::json!({
        "userStatus": {"cascadeModelConfigData": {"clientModelConfigs": []}}
    });
    let response: UserStatusResponse = serde_json::from_value(json).expect("fixture parses");
    let snapshot = AntigravityProvider::new()
        .parse_user_status(response)
        .expect("snapshot");

    assert_eq!(snapshot.primary.used_percent, 0.0);
    assert!(
        !snapshot.primary.quota_authoritative,
        "an empty model-config list is absent evidence, not a healthy quota"
    );
    assert_eq!(snapshot.primary.trusted_used_percent(), None);
    assert!(!has_trusted_live_quota(&snapshot));
}

#[test]
fn quota_configs_without_usable_values_yield_no_trusted_summary_lane() {
    let json = serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    {"label": "Claude 4 Sonnet", "quotaInfo": {}},
                    {"label": "Gemini 2.5 Pro Low", "quotaInfo": {"remainingFraction": null}},
                    {"label": "Gemini 2.5 Flash", "quotaInfo": {"remainingFraction": 1.5}}
                ]
            }
        }
    });
    let response: UserStatusResponse = serde_json::from_value(json).expect("fixture parses");
    let snapshot = AntigravityProvider::new()
        .parse_user_status(response)
        .expect("snapshot");

    assert_eq!(snapshot.primary.trusted_used_percent(), None);
    assert_eq!(
        snapshot
            .secondary
            .as_ref()
            .and_then(|window| window.trusted_used_percent()),
        None
    );
    assert_eq!(
        snapshot
            .model_specific
            .as_ref()
            .and_then(|window| window.trusted_used_percent()),
        None
    );
    assert!(
        !has_trusted_live_quota(&snapshot),
        "no bucket carried usable evidence, so no lane may claim live quota"
    );
}

#[test]
fn informational_placeholder_is_not_trusted_live_quota() {
    let snapshot = UsageSnapshot::new(RateWindow::no_active_session());
    assert!(!has_trusted_live_quota(&snapshot));

    let snapshot = UsageSnapshot::new(RateWindow::informational("Offline · 3 conversations"));
    assert!(!has_trusted_live_quota(&snapshot));
}

#[test]
fn trusted_live_quota_is_detected_in_every_snapshot_slot() {
    let trusted = || RateWindow::new(42.0);
    assert!(has_trusted_live_quota(&UsageSnapshot::new(trusted())));

    let untrusted = || RateWindow::new(0.0).non_authoritative();
    assert!(has_trusted_live_quota(
        &UsageSnapshot::new(untrusted()).with_secondary(trusted())
    ));
    assert!(has_trusted_live_quota(
        &UsageSnapshot::new(untrusted()).with_model_specific(trusted())
    ));
    assert!(has_trusted_live_quota(
        &UsageSnapshot::new(untrusted()).with_tertiary(trusted())
    ));
    assert!(has_trusted_live_quota(
        &UsageSnapshot::new(untrusted()).with_extra_rate_window("x", "X", trusted())
    ));
    assert!(!has_trusted_live_quota(
        &UsageSnapshot::new(untrusted()).with_extra_rate_window("x", "X", untrusted())
    ));
}

// ── Source resolution order ───────────────────────────────────────────────

/// Sanitized structured `agy /usage` report, matching the real host's shape.
const CLI_REPORT: &[u8] = br#"{
  "status": "SUCCESS",
  "command": {
    "name": "usage",
    "data": {
      "groups": [
        {
          "name": "Gemini Models",
          "buckets": [
            {"id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.85},
            {"id": "gemini-5h", "window": "5h", "remaining_fraction": 0.31}
          ]
        }
      ]
    }
  }
}"#;

fn cli_result() -> ProviderFetchResult {
    let usage = quota_summary::parse_cli_usage_report(CLI_REPORT).expect("CLI report parses");
    ProviderFetchResult::new(usage, "cli")
}

fn local_snapshot(fraction: Option<f64>) -> UsageSnapshot {
    let quota = match fraction {
        Some(value) => serde_json::json!({"remainingFraction": value}),
        None => serde_json::json!({}),
    };
    parse_single_quota(quota)
}

#[tokio::test]
async fn local_snapshot_without_trusted_quota_falls_through_to_the_cli_report() {
    let result = resolve_fetch_result(
        Ok(local_snapshot(None)),
        || async { Some(cli_result()) },
        || panic!("offline history must not be consulted once the CLI answered"),
    )
    .await
    .expect("CLI report resolves the fetch");

    assert_eq!(result.source_label, "cli");
    // The promoted lanes come from the CLI data, not the local placeholder.
    assert_eq!(
        result
            .usage
            .primary
            .authoritative_used_percent(RateWindowCadence::Session)
            .map(|used| (used * 1000.0).round() / 1000.0),
        Some(69.0)
    );
    assert_eq!(
        result
            .usage
            .secondary
            .as_ref()
            .and_then(|window| window.authoritative_used_percent(RateWindowCadence::Weekly))
            .map(|used| (used * 1000.0).round() / 1000.0),
        Some(15.0)
    );
}

#[tokio::test]
async fn local_snapshot_with_trusted_quota_wins_over_the_cli_report() {
    let result = resolve_fetch_result(
        Ok(local_snapshot(Some(0.8))),
        || async { panic!("a trusted local lane must not trigger the CLI fallback") },
        || panic!("offline history must not be consulted"),
    )
    .await
    .expect("local probe resolves the fetch");

    assert_eq!(result.source_label, "local");
    let used = result
        .usage
        .primary
        .trusted_used_percent()
        .expect("trusted local lane");
    assert!((used - 20.0).abs() < 0.001);
}

#[tokio::test]
async fn local_exhausted_zero_still_counts_as_trusted_local_quota() {
    let result = resolve_fetch_result(
        Ok(local_snapshot(Some(0.0))),
        || async { panic!("exhausted quota is evidence, not absence") },
        || panic!("offline history must not be consulted"),
    )
    .await
    .expect("local probe resolves the fetch");

    assert_eq!(result.source_label, "local");
    assert_eq!(result.usage.primary.trusted_used_percent(), Some(100.0));
}

#[tokio::test]
async fn untrusted_local_snapshot_is_the_last_resort_when_the_cli_is_unavailable() {
    let result = resolve_fetch_result(Ok(local_snapshot(None)), || async { None }, || 0)
        .await
        .expect("the untrusted local snapshot is still returned");

    assert_eq!(result.source_label, "local");
    assert_eq!(
        result.usage.primary.trusted_used_percent(),
        None,
        "it is returned for display only — it never regains authority"
    );
}

#[tokio::test]
async fn offline_history_answers_when_the_probe_and_the_cli_both_fail() {
    let result = resolve_fetch_result(
        Err(ProviderError::Other("probe failed".into())),
        || async { None },
        || 3,
    )
    .await
    .expect("offline history resolves the fetch");

    assert_eq!(result.source_label, "offline");
    assert_eq!(result.usage.login_method.as_deref(), Some("offline"));
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("Offline · 3 conversations")
    );
    assert!(result.usage.primary.is_informational);
    assert_eq!(result.usage.primary.trusted_used_percent(), None);
}

#[tokio::test]
async fn auth_required_survives_when_no_source_yields_live_quota() {
    let error = resolve_fetch_result(Err(ProviderError::AuthRequired), || async { None }, || 0)
        .await
        .expect_err("the actionable probe error is preserved");

    assert!(matches!(error, ProviderError::AuthRequired));
}

#[tokio::test]
async fn offline_history_outranks_an_actionable_probe_error() {
    // Existing intended order: offline history is preferred over surfacing the
    // probe error, and only an empty history lets `AuthRequired` through.
    let result = resolve_fetch_result(Err(ProviderError::AuthRequired), || async { None }, || 1)
        .await
        .expect("offline history resolves the fetch");

    assert_eq!(result.source_label, "offline");
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("Offline · 1 conversation")
    );
}

#[tokio::test]
async fn a_failed_probe_still_reaches_the_cli_report() {
    let result = resolve_fetch_result(
        Err(ProviderError::AuthRequired),
        || async { Some(cli_result()) },
        || panic!("offline history must not be consulted once the CLI answered"),
    )
    .await
    .expect("CLI report resolves the fetch");

    assert_eq!(result.source_label, "cli");
}
