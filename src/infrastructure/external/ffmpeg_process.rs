//! Runs and supervises the FFmpeg child process used for stream recording.
//!
//! This module owns process construction, cancellation, stall detection, and
//! stderr sanitization. It does not resolve stream URLs or know about HTTP.

use crate::infrastructure::InfrastructureError;
use std::future;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::watch;
use tokio::time::{Duration, Instant};

const SHUTDOWN_GRACE_SECS: u64 = 15;
const STALL_TIMEOUT_SECS: u64 = 120;
const STALL_CHECK_SECS: u64 = 5;

pub(super) async fn run_ffmpeg(
    ffmpeg_path: &Path,
    stream_url: &str,
    output_path: &Path,
    session_cookie: Option<&str>,
    max_duration_secs: Option<u64>,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<(), InfrastructureError> {
    let mut command = tokio::process::Command::new(ffmpeg_path);
    command.kill_on_drop(true);
    configure_process_isolation(&mut command);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    if let Some(cookie) = session_cookie {
        command
            .arg("-headers")
            .arg(format!("Cookie: {}\r\n", cookie));
    }

    let mut child = command
        .args(duration_args(max_duration_secs))
        .arg("-i")
        .arg(stream_url)
        .arg("-c")
        .arg("copy")
        .arg("-y")
        .arg(output_path)
        .spawn()
        .map_err(|e| {
            InfrastructureError::RecordingError(format!("Failed to start ffmpeg: {}", e))
        })?;
    let mut stderr_task = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut buffer = Vec::new();
            let _ = stderr.read_to_end(&mut buffer).await;
            buffer
        })
    });

    if let Some(mut cancel_rx) = cancel_rx {
        if *cancel_rx.borrow() {
            cancel_ffmpeg(&mut child, stderr_task.take()).await;
            return Err(InfrastructureError::RecordingCancelled);
        }

        tokio::select! {
            status = child.wait() => {
                check_exit_status(status, stderr_task.take()).await?;
            }
            _ = async { cancel_rx.wait_for(|value| *value).await.ok(); } => {
                cancel_ffmpeg(&mut child, stderr_task.take()).await;
                return Err(InfrastructureError::RecordingCancelled);
            }
            _ = wait_for_duration_limit(max_duration_secs) => {
                cancel_ffmpeg(&mut child, stderr_task.take()).await;
                return Err(InfrastructureError::RecordingCancelled);
            }
            _ = wait_for_stall(
                output_path,
                Duration::from_secs(STALL_TIMEOUT_SECS),
                Duration::from_secs(STALL_CHECK_SECS),
            ) => {
                cancel_ffmpeg(&mut child, stderr_task.take()).await;
                return Err(InfrastructureError::RecordingError(format!(
                    "FFmpeg no escribio datos nuevos durante {} segundos",
                    STALL_TIMEOUT_SECS
                )));
            }
        }
    } else {
        tokio::select! {
            status = child.wait() => {
                check_exit_status(status, stderr_task.take()).await?;
            }
            _ = wait_for_duration_limit(max_duration_secs) => {
                cancel_ffmpeg(&mut child, stderr_task.take()).await;
                return Err(InfrastructureError::RecordingCancelled);
            }
            _ = wait_for_stall(
                output_path,
                Duration::from_secs(STALL_TIMEOUT_SECS),
                Duration::from_secs(STALL_CHECK_SECS),
            ) => {
                cancel_ffmpeg(&mut child, stderr_task.take()).await;
                return Err(InfrastructureError::RecordingError(format!(
                    "FFmpeg no escribio datos nuevos durante {} segundos",
                    STALL_TIMEOUT_SECS
                )));
            }
        }
    }

    Ok(())
}

async fn check_exit_status(
    status: std::io::Result<ExitStatus>,
    stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>,
) -> Result<(), InfrastructureError> {
    let status = status.map_err(|e| {
        InfrastructureError::RecordingError(format!("Failed to wait for ffmpeg: {}", e))
    })?;
    if !status.success() {
        let stderr = read_stderr(stderr_task).await;
        return Err(InfrastructureError::RecordingError(format_ffmpeg_error(
            status, &stderr,
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn configure_process_isolation(command: &mut tokio::process::Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_isolation(_command: &mut tokio::process::Command) {}

fn duration_args(max_duration_secs: Option<u64>) -> Vec<String> {
    match max_duration_secs {
        Some(seconds) => vec!["-t".to_string(), seconds.to_string()],
        None => Vec::new(),
    }
}

async fn wait_for_duration_limit(max_duration_secs: Option<u64>) {
    match max_duration_secs {
        Some(seconds) => tokio::time::sleep(Duration::from_secs(seconds.saturating_add(10))).await,
        None => future::pending::<()>().await,
    }
}

async fn read_stderr(stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    match stderr_task {
        Some(task) => task.await.unwrap_or_default(),
        None => Vec::new(),
    }
}

async fn cancel_ffmpeg(child: &mut Child, stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>) {
    if !request_stdin_shutdown(child).await {
        request_process_shutdown(child);
    }
    if tokio::time::timeout(Duration::from_secs(SHUTDOWN_GRACE_SECS), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let _ = read_stderr(stderr_task).await;
}

async fn request_stdin_shutdown(child: &mut Child) -> bool {
    let Some(mut stdin) = child.stdin.take() else {
        return false;
    };

    stdin.write_all(b"q\n").await.is_ok()
}

async fn wait_for_stall(path: &Path, timeout: Duration, check_interval: Duration) {
    let mut last_size = file_size(path).await;
    let mut last_change = Instant::now();

    loop {
        tokio::time::sleep(check_interval).await;
        let size = file_size(path).await;
        if file_has_stalled(
            &mut last_size,
            &mut last_change,
            size,
            Instant::now(),
            timeout,
        ) {
            return;
        }
    }
}

fn file_has_stalled(
    last_size: &mut u64,
    last_change: &mut Instant,
    size: u64,
    now: Instant,
    timeout: Duration,
) -> bool {
    if size > *last_size {
        *last_size = size;
        *last_change = now;
        return false;
    }

    now.duration_since(*last_change) >= timeout
}

async fn file_size(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

#[cfg(unix)]
fn request_process_shutdown(child: &mut Child) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(pid as i32, libc::SIGINT);
        }
    } else {
        let _ = child.start_kill();
    }
}

#[cfg(not(unix))]
fn request_process_shutdown(child: &mut Child) {
    let _ = child.start_kill();
}

fn format_ffmpeg_error(status: ExitStatus, stderr: &[u8]) -> String {
    match summarize_stderr(stderr) {
        Some(stderr) => format!("FFmpeg exited with status: {}. stderr: {}", status, stderr),
        None => format!("FFmpeg exited with status: {}", status),
    }
}

fn summarize_stderr(stderr: &[u8]) -> Option<String> {
    const MAX_CHARS: usize = 1200;
    const MAX_LINES: usize = 8;

    let text = String::from_utf8_lossy(stderr);
    let mut summary = Vec::new();

    for line in text.lines() {
        let line = redact_sensitive_line(line.trim());
        if line.is_empty() {
            continue;
        }
        summary.push(line);
        if summary.len() >= MAX_LINES {
            break;
        }
    }

    let mut text = summary.join(" | ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > MAX_CHARS {
        text = text.chars().take(MAX_CHARS).collect();
        text.push_str("...");
    }
    Some(text)
}

fn redact_sensitive_line(line: &str) -> String {
    if line.to_ascii_lowercase().contains("cookie:") {
        "Cookie: [redacted]".to_string()
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    async fn executable_script(body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "cbrec_ffmpeg_test_{}.sh",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        tokio::fs::write(&path, format!("#!/bin/sh\n{body}\n"))
            .await
            .expect("write test script");
        let mut permissions = tokio::fs::metadata(&path)
            .await
            .expect("read script metadata")
            .permissions();
        permissions.set_mode(0o700);
        tokio::fs::set_permissions(&path, permissions)
            .await
            .expect("make test script executable");
        path
    }

    #[test]
    fn duration_arguments_include_ffmpeg_limit() {
        assert_eq!(
            duration_args(Some(20)),
            vec!["-t".to_string(), "20".to_string()]
        );
        assert!(duration_args(None).is_empty());
    }

    #[test]
    fn empty_stderr_has_no_summary() {
        assert_eq!(summarize_stderr(b"\n  \n"), None);
    }

    #[test]
    fn stderr_summary_redacts_cookie() {
        let summary = summarize_stderr(b"Cookie: PHPSESSID=secret; other=value\nfailure").unwrap();

        assert_eq!(summary, "Cookie: [redacted] | failure");
    }

    #[test]
    fn stderr_summary_limits_lines() {
        let stderr = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let summary = summarize_stderr(stderr).unwrap();

        assert_eq!(summary, "1 | 2 | 3 | 4 | 5 | 6 | 7 | 8");
    }

    #[test]
    fn stderr_summary_truncates_only_above_character_limit() {
        let exact_limit = "a".repeat(1200);
        let above_limit = "b".repeat(1201);

        assert_eq!(summarize_stderr(exact_limit.as_bytes()), Some(exact_limit));
        let truncated = summarize_stderr(above_limit.as_bytes()).unwrap();
        assert_eq!(truncated.chars().count(), 1203);
        assert!(truncated.ends_with("..."));
    }

    #[tokio::test]
    async fn stalled_file_is_detected() {
        let path = std::env::temp_dir().join(format!(
            "cbrec_stall_{}.part.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        tokio::fs::write(&path, b"data")
            .await
            .expect("create partial file");

        tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_stall(&path, Duration::from_millis(20), Duration::from_millis(5)),
        )
        .await
        .expect("stalled file must be detected within the test deadline");

        let _ = tokio::fs::remove_file(path).await;
    }

    #[test]
    fn file_growth_resets_stall_deadline() {
        let started = Instant::now();
        let timeout = Duration::from_millis(40);
        let mut last_size = 4;
        let mut last_change = started;

        assert!(!file_has_stalled(
            &mut last_size,
            &mut last_change,
            9,
            started + Duration::from_millis(25),
            timeout,
        ));
        assert!(!file_has_stalled(
            &mut last_size,
            &mut last_change,
            9,
            started + Duration::from_millis(64),
            timeout,
        ));
        assert!(file_has_stalled(
            &mut last_size,
            &mut last_change,
            9,
            started + Duration::from_millis(65),
            timeout,
        ));
    }

    #[tokio::test]
    async fn file_size_distinguishes_existing_and_missing_files() {
        let path = std::env::temp_dir().join(format!(
            "cbrec_size_{}.part.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));

        assert_eq!(file_size(&path).await, 0);
        tokio::fs::write(&path, b"1234")
            .await
            .expect("create sized file");
        assert_eq!(file_size(&path).await, 4);

        let _ = tokio::fs::remove_file(path).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdin_shutdown_sends_ffmpeg_quit_command() {
        let script = executable_script("read line; [ \"$line\" = q ]").await;
        let mut command = tokio::process::Command::new(&script);
        command.kill_on_drop(true).stdin(Stdio::piped());
        let mut child = command.spawn().expect("start test process");

        assert!(request_stdin_shutdown(&mut child).await);
        let status = tokio::time::timeout(Duration::from_millis(500), child.wait())
            .await
            .expect("process must receive stdin command")
            .expect("wait for test process");
        assert!(status.success());

        let _ = tokio::fs::remove_file(script).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_isolation_creates_a_process_group() {
        let script = executable_script("read line").await;
        let mut command = tokio::process::Command::new(&script);
        command.kill_on_drop(true).stdin(Stdio::piped());
        configure_process_isolation(&mut command);
        let mut child = command.spawn().expect("start isolated process");
        let pid = child.id().expect("child process id") as i32;

        assert_eq!(unsafe { libc::getpgid(pid) }, pid);

        let _ = child.kill().await;
        let _ = child.wait().await;
        let _ = tokio::fs::remove_file(script).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_shutdown_interrupts_the_child() {
        let mut command = tokio::process::Command::new("sleep");
        command.arg("30");
        command.kill_on_drop(true);
        let mut child = command.spawn().expect("start test process");

        request_process_shutdown(&mut child);
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("interrupt must stop the process")
            .expect("wait for interrupted process");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_waits_for_the_child_to_exit() {
        let marker = std::env::temp_dir().join(format!(
            "cbrec_shutdown_{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let body = format!(
            "trap 'printf signal > \"{}\"; exit 1' INT\nread line\nsleep 0.1\nprintf \"$line\" > \"{}\"",
            marker.display(),
            marker.display()
        );
        let script = executable_script(&body).await;
        let mut command = tokio::process::Command::new(&script);
        command.kill_on_drop(true).stdin(Stdio::piped());
        let mut child = command.spawn().expect("start test process");

        cancel_ffmpeg(&mut child, None).await;

        assert!(child.try_wait().expect("inspect child status").is_some());
        assert_eq!(
            tokio::fs::read_to_string(&marker)
                .await
                .expect("read shutdown marker"),
            "q"
        );
        let _ = tokio::fs::remove_file(script).await;
        let _ = tokio::fs::remove_file(marker).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn successful_process_returns_ok() {
        let script = executable_script("exit 0").await;
        let output = script.with_extension("mp4");

        let result = run_ffmpeg(
            &script,
            "https://example.com/live.m3u8",
            &output,
            None,
            None,
            None,
        )
        .await;

        assert!(result.is_ok());
        let _ = tokio::fs::remove_file(script).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_process_redacts_cookie_from_error() {
        let script = executable_script("echo 'Cookie: secret' >&2; echo failure >&2; exit 2").await;
        let output = script.with_extension("mp4");

        let error = run_ffmpeg(
            &script,
            "https://example.com/live.m3u8",
            &output,
            Some("secret"),
            None,
            None,
        )
        .await
        .expect_err("non-zero process must fail")
        .to_string();

        assert!(error.contains("Cookie: [redacted] | failure"));
        assert!(!error.contains("Cookie: secret"));
        let _ = tokio::fs::remove_file(script).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_cancelled_process_returns_cancelled() {
        let script = executable_script("read line; exit 0").await;
        let output = script.with_extension("mp4");
        let (cancel_tx, cancel_rx) = watch::channel(true);

        let result = run_ffmpeg(
            &script,
            "https://example.com/live.m3u8",
            &output,
            None,
            None,
            Some(cancel_rx),
        )
        .await;

        assert!(matches!(
            result,
            Err(InfrastructureError::RecordingCancelled)
        ));
        drop(cancel_tx);
        let _ = tokio::fs::remove_file(script).await;
    }

    #[tokio::test]
    async fn missing_executable_reports_start_failure() {
        let executable = std::env::temp_dir().join("cbrec_missing_ffmpeg_executable");
        let output = executable.with_extension("mp4");

        let error = run_ffmpeg(
            &executable,
            "https://example.com/live.m3u8",
            &output,
            None,
            None,
            None,
        )
        .await
        .expect_err("missing executable must fail")
        .to_string();

        assert!(error.contains("Failed to start ffmpeg"));
    }
}
