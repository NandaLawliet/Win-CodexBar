//! Bounded structured-CLI fallback for Antigravity usage.
//!
//! When the local language-server probe cannot serve live quota, a signed-in
//! `agy` CLI can still answer `/usage` with a structured JSON report. This
//! module locates that binary, gates it on a version that supports the report,
//! and runs it under strict bounds: no shell interpolation, null stdin,
//! discarded stderr, a size-capped stdout capture, a wall-clock timeout, and
//! kill-on-drop so a hung child never outlives the fetch.
//!
//! Nothing here logs credentials, tokens, or the child's environment.

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command as AsyncCommand;
use uuid::Uuid;

use super::quota_summary;
use crate::core::{ProviderError, ProviderFetchResult};

/// Hard kill bound for the usage report. Matches the CLI's own
/// `--print-timeout 90s` so we never cut the report short of its deadline; the
/// desktop shell applies its own, tighter per-provider refresh budget and
/// `kill_on_drop` reaps the child when that budget expires first.
const REPORT_TIMEOUT: Duration = Duration::from_secs(90);
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const REPORT_MAX_OUTPUT_BYTES: usize = 1_048_576;
const WORKDIR_CREATE_ATTEMPTS: usize = 8;

/// Structured `/usage` reports landed in agy 1.1.11.
const MINIMUM_STRUCTURED_USAGE_VERSION: (u64, u64, u64) = (1, 1, 11);

/// Scrubbed from the child environment: the CLI must fall back to its own
/// on-disk sign-in rather than inherit credentials from this process.
const OAUTH_CREDENTIALS_ENV: &str = "ANTIGRAVITY_OAUTH_CREDENTIALS_JSON";

/// Optional explicit override for the `agy` executable path.
const CLI_PATH_ENV: &str = "ANTIGRAVITY_CLI_PATH";

/// A private, per-invocation working directory for the child process.
#[derive(Debug)]
struct PrivateWorkdir {
    path: PathBuf,
}

impl PrivateWorkdir {
    fn create() -> Result<Self, ProviderError> {
        let temp_root = std::env::temp_dir();
        for _ in 0..WORKDIR_CREATE_ATTEMPTS {
            let path = temp_root.join(format!(
                "codexbar-agy-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;

                        if std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                            .is_err()
                        {
                            drop(std::fs::remove_dir(&path));
                            return Err(ProviderError::Other(
                                "Failed to prepare Antigravity CLI working directory".into(),
                            ));
                        }
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }

        Err(ProviderError::Other(
            "Failed to prepare Antigravity CLI working directory".into(),
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateWorkdir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct BoundedStdout {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

/// Read a child stdout stream incrementally, retaining only the configured
/// prefix while continuing to drain the pipe so the child cannot block on a
/// full stdout buffer.
async fn read_stdout_limited<R>(mut reader: R, max_bytes: usize) -> std::io::Result<BoundedStdout>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(max_bytes.min(8192));
    let mut chunk = [0_u8; 8192];
    let mut exceeded_limit = false;

    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }

        let remaining = max_bytes.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        if retained < read {
            exceeded_limit = true;
        }
    }

    Ok(BoundedStdout {
        bytes,
        exceeded_limit,
    })
}

/// Run the structured usage report, returning `None` when the CLI cannot serve
/// it. Failures are debug-logged and never surface CLI stderr.
pub(super) async fn try_fetch(binary: Option<PathBuf>) -> Option<ProviderFetchResult> {
    let binary = binary?;
    match fetch_structured_usage(&binary).await {
        Ok(usage) => Some(usage),
        Err(error) => {
            tracing::debug!(%error, "Antigravity structured CLI usage report unavailable");
            None
        }
    }
}

async fn fetch_structured_usage(binary: &Path) -> Result<ProviderFetchResult, ProviderError> {
    let version = run_cli_command(binary, &["--version"], VERSION_TIMEOUT).await?;
    if version.exceeded_limit {
        return Err(ProviderError::Parse(
            "Antigravity CLI version output is too large".into(),
        ));
    }
    if !is_supported_version(&String::from_utf8_lossy(&version.bytes)) {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage reports require agy 1.1.11 or later".into(),
        ));
    }

    let output = run_cli_command(
        binary,
        &[
            "-p",
            "/usage",
            "--output-format",
            "json",
            "--print-timeout",
            "90s",
        ],
        REPORT_TIMEOUT,
    )
    .await?;
    if output.exceeded_limit {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage report is too large".into(),
        ));
    }

    let usage = quota_summary::parse_cli_usage_report(&output.bytes)?;
    Ok(ProviderFetchResult::new(usage, "cli"))
}

/// Build the child command: the executable is spawned directly with an
/// argument array (no shell), stdin is closed, and stderr is discarded so CLI
/// diagnostics can never leak into provider output.
fn prepare_command(binary: &Path, args: &[&str], working_dir: &Path) -> AsyncCommand {
    let mut command = AsyncCommand::new(binary);
    command
        .args(args)
        .current_dir(working_dir)
        .env_remove(OAUTH_CREDENTIALS_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.as_std_mut().creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    command
}

async fn run_cli_command(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<BoundedStdout, ProviderError> {
    let working_dir = PrivateWorkdir::create()?;
    let mut command = prepare_command(binary, args, working_dir.path());

    let mut child = command
        .spawn()
        .map_err(|_| ProviderError::Other("Failed to start Antigravity CLI".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::Other("Antigravity CLI usage report failed".into()))?;

    tokio::time::timeout(timeout, async {
        let (stdout_result, status_result) = tokio::join!(
            read_stdout_limited(stdout, REPORT_MAX_OUTPUT_BYTES),
            child.wait()
        );
        let stdout = stdout_result
            .map_err(|_| ProviderError::Other("Antigravity CLI usage report failed".into()))?;
        let status = status_result
            .map_err(|_| ProviderError::Other("Antigravity CLI usage report failed".into()))?;
        if !status.success() {
            return Err(ProviderError::Other(
                "Antigravity CLI usage report failed".into(),
            ));
        }
        Ok(stdout)
    })
    .await
    .map_err(|_| ProviderError::Timeout)?
}

/// Resolve the `agy` executable from the explicit override, `PATH`, and the
/// standard per-user install locations.
pub(super) fn locate_agy_binary() -> Option<PathBuf> {
    agy_binary_candidates(
        std::env::var_os(CLI_PATH_ENV).map(PathBuf::from),
        which::which("agy").ok(),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        dirs::home_dir(),
    )
    .into_iter()
    .find(|path| path.is_file())
}

fn agy_binary_candidates(
    explicit: Option<PathBuf>,
    path_lookup: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let executable = if cfg!(windows) { "agy.exe" } else { "agy" };
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        candidates.push(path);
    }
    if let Some(path) = path_lookup {
        candidates.push(path);
    }
    if let Some(root) = local_app_data {
        candidates.push(root.join("agy").join("bin").join("agy.exe"));
    }
    if let Some(root) = home {
        candidates.push(root.join(".local").join("bin").join(executable));
    }
    candidates
}

fn is_supported_version(raw: &str) -> bool {
    parse_cli_version(raw).is_some_and(|version| version >= MINIMUM_STRUCTURED_USAGE_VERSION)
}

/// Extract `major.minor.patch` from `agy --version` output.
///
/// The CLI prints a bare version line today; a leading program name
/// (`agy 1.2.6`) is tolerated. Only strict all-digit triples are accepted, so a
/// pre-release or build suffix reads as an unknown version and fails closed.
fn parse_cli_version(raw: &str) -> Option<(u64, u64, u64)> {
    raw.split_whitespace().find_map(parse_version_triple)
}

fn parse_version_triple(token: &str) -> Option<(u64, u64, u64)> {
    let mut parts = token.split('.');
    let major = parse_version_part(parts.next()?)?;
    let minor = parse_version_part(parts.next()?)?;
    let patch = parse_version_part(parts.next()?)?;
    parts.next().is_none().then_some((major, minor, patch))
}

fn parse_version_part(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse::<u64>().ok()
}

#[cfg(test)]
#[path = "cli_fallback_tests.rs"]
mod tests;
