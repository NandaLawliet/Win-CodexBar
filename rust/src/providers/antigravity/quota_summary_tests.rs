use super::*;
use crate::core::RateWindowCadence;

/// Sanitized shape of a real signed-in `agy /usage` response: two model-family
/// groups, each with a weekly and a 5-hour bucket. The fractions are round
/// stand-ins, not any particular account's readings.
const HOST_SHAPED_REPORT: &[u8] = br#"{
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
        },
        {
          "name": "Claude and GPT models",
          "buckets": [
            {"id": "third-party-weekly", "window": "weekly", "remaining_fraction": 1.0},
            {"id": "third-party-5h", "window": "5h", "remaining_fraction": 1.0}
          ]
        }
      ]
    }
  }
}"#;

fn report(body: &str) -> Vec<u8> {
    format!(r#"{{"status":"SUCCESS","command":{{"name":"usage","data":{body}}}}}"#).into_bytes()
}

fn find_window<'a>(snapshot: &'a UsageSnapshot, title: &str) -> &'a NamedRateWindow {
    snapshot
        .extra_rate_windows
        .iter()
        .find(|window| window.title == title)
        .unwrap_or_else(|| panic!("missing lane {title}"))
}

// ── Positive parse: lane mapping, cadence, and derived percentages ──────────

#[test]
fn parses_host_shaped_report_into_gemini_summary_lanes() {
    let snapshot = parse_cli_usage_report(HOST_SHAPED_REPORT).expect("CLI report parses");

    // Gemini 5h drives the primary lane: 0.31 remaining → 69% used.
    assert!((snapshot.primary.used_percent - 69.0).abs() < 0.001);
    assert_eq!(snapshot.primary.window_minutes, Some(300));
    assert_eq!(
        snapshot
            .primary
            .authoritative_used_percent(RateWindowCadence::Session)
            .map(|used| (used * 1000.0).round() / 1000.0),
        Some(69.0)
    );

    // Gemini weekly drives the secondary lane: 0.85 remaining → 15% used.
    let weekly = snapshot.secondary.as_ref().expect("weekly lane");
    assert!((weekly.used_percent - 15.0).abs() < 0.001);
    assert_eq!(weekly.window_minutes, Some(10_080));
    assert_eq!(
        weekly
            .authoritative_used_percent(RateWindowCadence::Weekly)
            .map(|used| (used * 1000.0).round() / 1000.0),
        Some(15.0)
    );
}

#[test]
fn third_party_buckets_stay_extra_lanes_with_group_labels() {
    let snapshot = parse_cli_usage_report(HOST_SHAPED_REPORT).expect("CLI report parses");

    let titles: Vec<&str> = snapshot
        .extra_rate_windows
        .iter()
        .map(|window| window.title.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "Gemini 5h",
            "Gemini Weekly",
            "Claude and GPT models 5h",
            "Claude and GPT models Weekly",
        ]
    );

    for title in ["Claude and GPT models 5h", "Claude and GPT models Weekly"] {
        let lane = find_window(&snapshot, title);
        assert!(lane.usage_known, "{title} reported a usable fraction");
        assert_eq!(lane.window.used_percent, 0.0, "{title}: 1.0 remaining");
        assert_eq!(
            lane.window.trusted_used_percent(),
            Some(0.0),
            "{title}: a real 1.0 reading is authoritative 0% used"
        );
    }

    assert_eq!(
        find_window(&snapshot, "Claude and GPT models 5h")
            .window
            .window_minutes,
        Some(300)
    );
    assert_eq!(
        find_window(&snapshot, "Claude and GPT models Weekly")
            .window
            .window_minutes,
        Some(10_080)
    );
}

#[test]
fn boundary_fractions_stay_authoritative() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.0},
            {"id":"gemini-weekly","window":"weekly","remaining_fraction":1.0}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert_eq!(
        snapshot
            .primary
            .authoritative_used_percent(RateWindowCadence::Session),
        Some(100.0),
        "0.0 remaining is a real, fully-consumed quota"
    );
    assert_eq!(
        snapshot
            .secondary
            .expect("weekly")
            .authoritative_used_percent(RateWindowCadence::Weekly),
        Some(0.0),
        "1.0 remaining is a real, untouched quota"
    );
}

#[test]
fn cadence_is_read_from_explicit_labels_not_array_order() {
    // The weekly bucket is listed first and the ids carry no cadence hint; only
    // the explicit `window` labels distinguish the lanes.
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"bucket-a","window":"weekly","remaining_fraction":0.4},
            {"id":"bucket-b","window":"5h","remaining_fraction":0.9}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert_eq!(snapshot.primary.window_minutes, Some(300));
    assert!((snapshot.primary.used_percent - 10.0).abs() < 0.001);
    let weekly = snapshot.secondary.expect("weekly");
    assert_eq!(weekly.window_minutes, Some(10_080));
    assert!((weekly.used_percent - 60.0).abs() < 0.001);
}

#[test]
fn cadence_labels_tolerate_decorated_display_names() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"a","name":"Five Hour Limit Remaining","remaining_fraction":0.6},
            {"id":"b","name":"Weekly Limit Remaining","remaining_fraction":0.8}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert_eq!(snapshot.primary.window_minutes, Some(300));
    assert!((snapshot.primary.used_percent - 40.0).abs() < 0.001);
    assert_eq!(
        snapshot.secondary.expect("weekly").window_minutes,
        Some(10_080)
    );
}

#[test]
fn reset_time_is_carried_onto_the_lane() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.6,"reset_time":"2026-09-20T12:34:56Z"}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert_eq!(
        snapshot.primary.resets_at.map(|value| value.to_rfc3339()),
        Some("2026-09-20T12:34:56+00:00".to_string())
    );
}

#[test]
fn nested_remaining_oneof_is_resolved() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining":{"case":"remainingFraction","value":0.25}}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!((snapshot.primary.used_percent - 75.0).abs() < 0.001);
    assert_eq!(
        snapshot
            .primary
            .authoritative_used_percent(RateWindowCadence::Session),
        Some(75.0)
    );
}

// ── Envelope rejection ──────────────────────────────────────────────────────

#[test]
fn rejects_non_success_status() {
    let data = br#"{"status":"ERROR","command":{"name":"usage","data":{"groups":[
        {"name":"Gemini Models","buckets":[{"id":"gemini-5h","window":"5h","remaining_fraction":0.5}]}
    ]}}}"#;

    assert!(parse_cli_usage_report(data).is_err());
}

#[test]
fn rejects_unrelated_command_name() {
    let data = br#"{"status":"SUCCESS","command":{"name":"models","data":{"groups":[
        {"name":"Gemini Models","buckets":[{"id":"gemini-5h","window":"5h","remaining_fraction":0.5}]}
    ]}}}"#;

    assert!(parse_cli_usage_report(data).is_err());
}

#[test]
fn rejects_missing_groups() {
    assert!(
        parse_cli_usage_report(br#"{"status":"SUCCESS","command":{"name":"usage","data":{}}}"#)
            .is_err()
    );
    assert!(parse_cli_usage_report(&report(r#"{"groups":[]}"#)).is_err());
}

#[test]
fn rejects_group_without_buckets() {
    assert!(parse_cli_usage_report(&report(r#"{"groups":[{"name":"Gemini Models"}]}"#)).is_err());
    assert!(
        parse_cli_usage_report(&report(
            r#"{"groups":[{"name":"Gemini Models","buckets":[]}]}"#
        ))
        .is_err()
    );
}

#[test]
fn rejects_malformed_json() {
    assert!(parse_cli_usage_report(b"not json at all").is_err());
    assert!(parse_cli_usage_report(br#"{"status":"SUCCESS","command":"#).is_err());
    assert!(parse_cli_usage_report(b"").is_err());
}

#[test]
fn rejects_a_plain_agent_turn_that_never_ran_the_slash_command() {
    // When `/usage` is not recognized as a slash command the CLI answers the
    // prompt as an ordinary agent turn: same `"status":"SUCCESS"`, but no
    // `command` envelope. That carries no quota evidence and must be rejected
    // rather than read as an empty — and therefore untouched — quota.
    let data = br#"{
      "conversation_id": "00000000-0000-0000-0000-000000000000",
      "status": "SUCCESS",
      "response": "Here is a breakdown of disk usage...",
      "duration_seconds": 89.3,
      "num_turns": 1,
      "usage": {"input_tokens": 1, "output_tokens": 2, "total_tokens": 3}
    }"#;

    assert!(parse_cli_usage_report(data).is_err());
}

#[test]
fn integer_remaining_fractions_parse_as_fractions() {
    // The CLI emits an untouched bucket as JSON `1`, not `1.0`.
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":1},
            {"id":"gemini-weekly","window":"weekly","remaining_fraction":0}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert_eq!(
        snapshot
            .primary
            .authoritative_used_percent(RateWindowCadence::Session),
        Some(0.0)
    );
    assert_eq!(
        snapshot
            .secondary
            .expect("weekly")
            .authoritative_used_percent(RateWindowCadence::Weekly),
        Some(100.0)
    );
}

#[test]
fn parses_the_real_bucket_field_set_emitted_by_agy_1_2_6() {
    // Field-for-field shape of a live `agy 1.2.6` report (`id`, `name`,
    // `description`, `window`, `remaining_fraction`, `reset_time`), with
    // stand-in fractions.
    let data = br#"{
      "conversation_id": "00000000-0000-0000-0000-000000000000",
      "status": "SUCCESS",
      "response": "",
      "num_turns": 1,
      "command": {
        "name": "usage",
        "data": {
          "description": "Usage limits",
          "groups": [
            {
              "name": "Gemini Models",
              "description": "Gemini usage",
              "buckets": [
                {"id":"gemini-weekly","name":"Weekly Limit Remaining","description":"resets weekly","window":"weekly","remaining_fraction":0.5,"reset_time":"2026-09-24T18:53:31Z"},
                {"id":"gemini-5h","name":"Five Hour Limit Remaining","description":"resets every 5h","window":"5h","remaining_fraction":0.25,"reset_time":"2026-09-18T10:54:20Z"}
              ]
            },
            {
              "name": "Claude and GPT models",
              "description": "Third-party usage",
              "buckets": [
                {"id":"3p-weekly","name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":1,"reset_time":"2026-09-25T09:01:18Z"},
                {"id":"3p-5h","name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":1,"reset_time":"2026-09-18T14:01:18Z"}
              ]
            }
          ]
        }
      }
    }"#;

    let snapshot = parse_cli_usage_report(data).expect("CLI report parses");

    assert_eq!(
        snapshot
            .primary
            .authoritative_used_percent(RateWindowCadence::Session),
        Some(75.0)
    );
    assert_eq!(
        snapshot
            .secondary
            .as_ref()
            .and_then(|weekly| weekly.authoritative_used_percent(RateWindowCadence::Weekly)),
        Some(50.0)
    );
    let titles: Vec<&str> = snapshot
        .extra_rate_windows
        .iter()
        .map(|window| window.title.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "Gemini 5h",
            "Gemini Weekly",
            "Claude and GPT models 5h",
            "Claude and GPT models Weekly",
        ]
    );
    assert_eq!(
        snapshot.primary.resets_at.map(|value| value.to_rfc3339()),
        Some("2026-09-18T10:54:20+00:00".to_string())
    );
}

#[test]
fn rejects_a_report_whose_every_bucket_is_unusable() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h"},
            {"id":"gemini-weekly","window":"weekly","remaining_fraction":1.5}
        ]}]}"#,
    );

    assert!(
        parse_cli_usage_report(&data).is_err(),
        "no usable evidence must surface as unavailable, not as a healthy snapshot"
    );
}

#[test]
fn ignores_unknown_extra_fields() {
    let data = br#"{
      "status": "SUCCESS",
      "requestId": "abc",
      "command": {
        "name": "usage",
        "exitCode": 0,
        "data": {
          "description": "Usage",
          "groups": [{
            "name": "Gemini Models",
            "tint": "blue",
            "buckets": [
              {"id":"gemini-5h","window":"5h","remaining_fraction":0.5,"futureField":{"a":1}}
            ]
          }]
        }
      }
    }"#;

    let snapshot = parse_cli_usage_report(data).expect("unknown fields are ignored");
    assert!((snapshot.primary.used_percent - 50.0).abs() < 0.001);
}

// ── Quota authority: invalid evidence never becomes a favorable reading ─────

#[test]
fn missing_remaining_fraction_is_non_authoritative() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.4},
            {"id":"gemini-weekly","window":"weekly"}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!(
        snapshot.secondary.is_none(),
        "an evidence-free weekly bucket must not be promoted to a summary lane"
    );
    let weekly = find_window(&snapshot, "Gemini Weekly");
    assert!(!weekly.usage_known);
    assert_eq!(
        weekly.window.trusted_used_percent(),
        None,
        "a missing fraction must not read as 0% used / 100% remaining"
    );
}

#[test]
fn explicit_null_remaining_fraction_is_non_authoritative() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.4},
            {"id":"gemini-weekly","window":"weekly","remaining_fraction":null}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    let weekly = find_window(&snapshot, "Gemini Weekly");
    assert!(!weekly.usage_known);
    assert_eq!(weekly.window.trusted_used_percent(), None);
}

#[test]
fn out_of_range_remaining_fractions_are_non_authoritative() {
    for fraction in ["-0.1", "1.1", "-1.0", "2.0"] {
        let data = report(&format!(
            r#"{{"groups":[{{"name":"Gemini Models","buckets":[
                {{"id":"gemini-5h","window":"5h","remaining_fraction":0.4}},
                {{"id":"gemini-weekly","window":"weekly","remaining_fraction":{fraction}}}
            ]}}]}}"#
        ));

        let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

        let weekly = find_window(&snapshot, "Gemini Weekly");
        assert!(
            !weekly.usage_known,
            "{fraction} is out of range and carries no evidence"
        );
        assert_eq!(
            weekly.window.trusted_used_percent(),
            None,
            "{fraction} must not be clamped into an authoritative percentage"
        );
        assert!(
            snapshot.secondary.is_none(),
            "{fraction} must not be promoted to the weekly summary lane"
        );
    }
}

#[test]
fn authority_is_decided_on_the_raw_fraction_not_a_clamped_one() {
    // The laundering path under test: 1.1 → clamp(0..1) → 1.0 → "0% used".
    assert_eq!(authoritative_used_percent(Some(1.1)), None);
    assert_eq!(authoritative_used_percent(Some(-0.1)), None);
    assert_eq!(authoritative_used_percent(Some(f64::NAN)), None);
    assert_eq!(authoritative_used_percent(Some(f64::INFINITY)), None);
    assert_eq!(authoritative_used_percent(Some(f64::NEG_INFINITY)), None);
    assert_eq!(authoritative_used_percent(None), None);

    assert_eq!(authoritative_used_percent(Some(1.0)), Some(0.0));
    assert_eq!(authoritative_used_percent(Some(0.0)), Some(100.0));
    assert_eq!(authoritative_used_percent(Some(0.5)), Some(50.0));
}

#[test]
fn every_derived_percent_stays_inside_the_trustworthy_band() {
    for step in 0..=1000_u32 {
        let fraction = f64::from(step) / 1000.0;
        let used = authoritative_used_percent(Some(fraction)).expect("in-range fraction");
        assert!(
            used.is_finite() && (0.0..=100.0).contains(&used),
            "fraction {fraction} produced {used}"
        );
        assert_eq!(
            RateWindow::with_details(used, Some(SESSION_WINDOW_MINUTES), None, None)
                .trusted_used_percent(),
            Some(used),
            "fraction {fraction} must survive RateWindow's own authority gate"
        );
    }
}

#[test]
fn disabled_buckets_carry_no_authority() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.4},
            {"id":"gemini-weekly","window":"weekly","disabled":true,"remaining_fraction":1.0}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!(snapshot.secondary.is_none());
    let weekly = find_window(&snapshot, "Gemini Weekly");
    assert!(!weekly.usage_known);
    assert_eq!(weekly.window.trusted_used_percent(), None);
}

#[test]
fn absent_session_bucket_yields_an_informational_primary() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-weekly","window":"weekly","remaining_fraction":0.8}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!(snapshot.primary.is_informational);
    assert_eq!(snapshot.primary.trusted_used_percent(), None);
    assert!((snapshot.secondary.expect("weekly").used_percent - 20.0).abs() < 0.001);
}

#[test]
fn third_party_cadence_never_substitutes_for_a_missing_gemini_lane() {
    let data = report(
        r#"{"groups":[
            {"name":"Gemini Models","buckets":[
                {"id":"gemini-5h","window":"5h","remaining_fraction":0.9}
            ]},
            {"name":"Claude and GPT models","buckets":[
                {"id":"third-party-weekly","window":"weekly","remaining_fraction":0.2}
            ]}
        ]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!((snapshot.primary.used_percent - 10.0).abs() < 0.001);
    assert!(
        snapshot.secondary.is_none(),
        "the Gemini group owns the weekly lane even when it omits it"
    );
    assert_eq!(
        find_window(&snapshot, "Claude and GPT models Weekly")
            .window
            .used_percent,
        80.0
    );
}

#[test]
fn third_party_group_supplies_summary_lanes_when_no_gemini_group_exists() {
    let data = report(
        r#"{"groups":[{"name":"Claude and GPT models","buckets":[
            {"id":"third-party-5h","window":"5h","remaining_fraction":0.7},
            {"id":"third-party-weekly","window":"weekly","remaining_fraction":0.6}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!((snapshot.primary.used_percent - 30.0).abs() < 0.001);
    assert!((snapshot.secondary.expect("weekly").used_percent - 40.0).abs() < 0.001);
}

// ── Unknown / duplicate structures ──────────────────────────────────────────

#[test]
fn unknown_group_stays_an_extra_lane() {
    let data = report(
        r#"{"groups":[
            {"name":"Gemini Models","buckets":[
                {"id":"gemini-5h","window":"5h","remaining_fraction":0.9}
            ]},
            {"name":"Experimental","buckets":[
                {"id":"experimental-daily","window":"daily","remaining_fraction":0.4}
            ]}
        ]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!((snapshot.primary.used_percent - 10.0).abs() < 0.001);
    let unknown = find_window(&snapshot, "Experimental experimental-daily");
    assert_eq!(
        unknown.window.window_minutes, None,
        "an unrecognized cadence gets no fabricated window length"
    );
    assert_eq!(
        unknown
            .window
            .authoritative_used_percent(RateWindowCadence::Session),
        None
    );
    assert_eq!(
        unknown
            .window
            .authoritative_used_percent(RateWindowCadence::Weekly),
        None
    );
}

#[test]
fn unknown_bucket_cadence_never_reaches_a_summary_lane() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-monthly","window":"monthly","remaining_fraction":0.5}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert!(snapshot.primary.is_informational);
    assert!(snapshot.secondary.is_none());
    assert_eq!(snapshot.extra_rate_windows.len(), 1);
}

#[test]
fn buckets_without_an_identifier_are_skipped() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"window":"weekly","remaining_fraction":0.5},
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.5}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    assert_eq!(snapshot.extra_rate_windows.len(), 1);
    assert!(snapshot.secondary.is_none());
}

#[test]
fn duplicate_bucket_ids_within_a_group_stay_distinct() {
    let data = report(
        r#"{"groups":[{"name":"Gemini Models","buckets":[
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.5},
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.2}
        ]}]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    let ids: Vec<&str> = snapshot
        .extra_rate_windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "duplicate bucket ids must not collide");
    // The most-constrained 5h lane wins the primary slot.
    assert!((snapshot.primary.used_percent - 80.0).abs() < 0.001);
}

#[test]
fn duplicate_bucket_ids_across_groups_stay_distinct() {
    let data = report(
        r#"{"groups":[
            {"name":"Gemini Models","buckets":[
                {"id":"weekly","window":"weekly","remaining_fraction":0.8}
            ]},
            {"name":"Claude and GPT models","buckets":[
                {"id":"weekly","window":"weekly","remaining_fraction":0.4}
            ]}
        ]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    let ids: Vec<&str> = snapshot
        .extra_rate_windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!((snapshot.secondary.expect("weekly").used_percent - 20.0).abs() < 0.001);
}

#[test]
fn repeated_group_names_get_distinct_window_id_scopes() {
    let data = report(
        r#"{"groups":[
            {"name":"Gemini Models","buckets":[
                {"id":"5h","window":"5h","remaining_fraction":0.9}
            ]},
            {"name":"Gemini Models","buckets":[
                {"id":"5h","window":"5h","remaining_fraction":0.8}
            ]}
        ]}"#,
    );

    let snapshot = parse_cli_usage_report(&data).expect("CLI report parses");

    let ids: Vec<&str> = snapshot
        .extra_rate_windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

// ── Public contract ─────────────────────────────────────────────────────────

#[test]
fn cli_snapshot_public_json_schema_is_unchanged() {
    let snapshot = parse_cli_usage_report(HOST_SHAPED_REPORT).expect("CLI report parses");
    let value = serde_json::to_value(&snapshot).expect("serialize");

    let mut keys: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["extra_rate_windows", "primary", "secondary", "updated_at",]
    );

    let mut window_keys: Vec<&str> = value["primary"]
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    window_keys.sort_unstable();
    assert_eq!(
        window_keys,
        ["is_informational", "used_percent", "window_minutes"]
    );

    let mut named_keys: Vec<&str> = value["extra_rate_windows"][0]
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    named_keys.sort_unstable();
    assert_eq!(named_keys, ["id", "title", "window"]);
}

#[test]
fn authority_does_not_survive_the_public_json_round_trip() {
    let snapshot = parse_cli_usage_report(HOST_SHAPED_REPORT).expect("CLI report parses");
    assert!(snapshot.primary.trusted_used_percent().is_some());

    let json = serde_json::to_string(&snapshot).expect("serialize");
    let restored: UsageSnapshot = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.primary.used_percent, snapshot.primary.used_percent);
    assert_eq!(
        restored.primary.trusted_used_percent(),
        None,
        "authority is granted in-process, never rebuilt from bytes"
    );
}
