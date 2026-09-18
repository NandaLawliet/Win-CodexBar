use super::*;

// ── Version gate ────────────────────────────────────────────────────────────

#[test]
fn structured_usage_requires_agy_1_1_11_or_later() {
    for supported in ["1.1.11", "1.1.12", "1.2.0", "1.2.6", "2.0.0", "10.0.0"] {
        assert!(
            is_supported_version(supported),
            "{supported} supports structured /usage"
        );
    }

    for unsupported in ["1.1.10", "1.1.0", "1.0.99", "0.9.9"] {
        assert!(
            !is_supported_version(unsupported),
            "{unsupported} predates structured /usage"
        );
    }
}

#[test]
fn unparseable_version_output_fails_closed() {
    for raw in [
        "",
        "   ",
        "unknown",
        "1.2",
        "1.2.6.3",
        "1.2.2-preview",
        "+1.2.2",
        "1.2.x",
        "v1.2.6",
    ] {
        assert!(
            !is_supported_version(raw),
            "{raw:?} is not a recognized version and must not unlock the report"
        );
    }
}

#[test]
fn version_line_may_carry_a_leading_program_name() {
    assert!(is_supported_version("1.2.6\n"));
    assert!(is_supported_version("agy 1.2.6"));
    assert!(is_supported_version("agy version 1.2.6\n"));
    assert!(!is_supported_version("agy version 1.1.10\n"));
}

// ── Bounded stdout capture ──────────────────────────────────────────────────

#[tokio::test]
async fn stdout_capture_retains_only_the_configured_limit() {
    let output = read_stdout_limited(b"0123456789".as_slice(), 4)
        .await
        .expect("stdout reader should succeed");
    assert_eq!(output.bytes, b"0123");
    assert!(output.exceeded_limit);

    let output = read_stdout_limited(b"0123".as_slice(), 4)
        .await
        .expect("stdout reader should succeed");
    assert_eq!(output.bytes, b"0123");
    assert!(!output.exceeded_limit);
}

#[tokio::test]
async fn oversized_stdout_is_flagged_and_capped_at_the_report_limit() {
    let oversized = vec![b'x'; REPORT_MAX_OUTPUT_BYTES + 4096];

    let output = read_stdout_limited(oversized.as_slice(), REPORT_MAX_OUTPUT_BYTES)
        .await
        .expect("stdout reader should succeed");

    assert_eq!(output.bytes.len(), REPORT_MAX_OUTPUT_BYTES);
    assert!(
        output.exceeded_limit,
        "an oversized report must be rejected, not silently truncated into a parse"
    );
}

#[tokio::test]
async fn stdout_capture_drains_the_whole_stream_past_the_limit() {
    // The pipe must keep draining so the child never blocks on a full buffer.
    let stream = vec![b'y'; 40_000];

    let output = read_stdout_limited(stream.as_slice(), 16)
        .await
        .expect("stdout reader should succeed");

    assert_eq!(output.bytes.len(), 16);
    assert!(output.exceeded_limit);
}

// ── Subprocess hardening ────────────────────────────────────────────────────

#[test]
fn subprocess_uses_a_private_workdir_and_scrubs_oauth_credentials() {
    let workdir = PrivateWorkdir::create().expect("private working directory");
    let path = workdir.path().to_path_buf();
    assert!(path.is_dir());
    assert_ne!(path, std::env::temp_dir());

    {
        let command = prepare_command(Path::new("agy"), &["--version"], workdir.path());
        let standard_command = command.as_std();

        assert_eq!(standard_command.get_current_dir(), Some(workdir.path()));
        assert!(standard_command.get_envs().any(|(key, value)| {
            key == std::ffi::OsStr::new(OAUTH_CREDENTIALS_ENV) && value.is_none()
        }));
    }

    drop(workdir);
    assert!(!path.exists());
}

#[test]
fn structured_usage_is_spawned_as_an_argument_array_without_a_shell() {
    let workdir = PrivateWorkdir::create().expect("private working directory");
    let command = prepare_command(
        Path::new("agy"),
        &[
            "-p",
            "/usage",
            "--output-format",
            "json",
            "--print-timeout",
            "90s",
        ],
        workdir.path(),
    );
    let standard_command = command.as_std();

    assert_eq!(standard_command.get_program(), std::ffi::OsStr::new("agy"));
    let args: Vec<_> = standard_command.get_args().collect();
    assert_eq!(
        args,
        [
            "-p",
            "/usage",
            "--output-format",
            "json",
            "--print-timeout",
            "90s"
        ]
        .map(std::ffi::OsStr::new)
    );
}

#[tokio::test]
async fn a_missing_binary_yields_no_result_instead_of_an_error() {
    let missing = std::env::temp_dir().join("codexbar-agy-does-not-exist.exe");

    assert!(try_fetch(None).await.is_none());
    assert!(try_fetch(Some(missing)).await.is_none());
}

// ── Binary discovery ────────────────────────────────────────────────────────

#[test]
fn agy_candidates_are_ordered_override_then_path_then_install_roots() {
    let candidates = agy_binary_candidates(
        Some(PathBuf::from("/override/agy")),
        Some(PathBuf::from("/path/agy")),
        Some(PathBuf::from("C:/Users/test/AppData/Local")),
        Some(PathBuf::from("C:/Users/test")),
    );

    assert_eq!(candidates[0], PathBuf::from("/override/agy"));
    assert_eq!(candidates[1], PathBuf::from("/path/agy"));
    assert_eq!(
        candidates[2],
        PathBuf::from("C:/Users/test/AppData/Local/agy/bin/agy.exe")
    );
    assert_eq!(
        candidates[3],
        PathBuf::from("C:/Users/test")
            .join(".local")
            .join("bin")
            .join(if cfg!(windows) { "agy.exe" } else { "agy" })
    );
}

#[test]
fn agy_candidates_skip_unavailable_roots() {
    assert!(agy_binary_candidates(None, None, None, None).is_empty());
    assert_eq!(
        agy_binary_candidates(None, Some(PathBuf::from("/path/agy")), None, None),
        vec![PathBuf::from("/path/agy")]
    );
}
