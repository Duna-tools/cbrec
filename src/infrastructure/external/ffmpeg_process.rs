//! Supervisa FFmpeg, la recuperación acotada de red y el cierre de grabaciones.
//!
//! Conserva los diagnósticos finales sin exponer URLs multimedia ni cookies.

use crate::infrastructure::InfrastructureError;
use std::collections::VecDeque;
use std::future;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::watch;
use tokio::time::{Duration, Instant};

const SHUTDOWN_GRACE_SECS: u64 = 15;
const STALL_TIMEOUT_SECS: u64 = 120;
const STALL_CHECK_SECS: u64 = 5;
const DISK_CHECK_SECS: u64 = 30;
const MAX_FFMPEG_STDERR_BYTES: usize = 64 * 1024;

async fn read_bounded_stderr<R>(reader: &mut R, input_damaged: &AtomicBool) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut retained = VecDeque::with_capacity(MAX_FFMPEG_STDERR_BYTES);
    let mut chunk = [0_u8; 8 * 1024];
    let mut detection = Vec::with_capacity(chunk.len() + 64);

    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        if !input_damaged.load(Ordering::Relaxed) {
            detection.extend_from_slice(&chunk[..read]);
            let damaged = [
                b"Packet corrupt".as_slice(),
                b"corrupt input packet",
                b"failed too many times, skipping",
            ]
            .iter()
            .any(|marker| detection.windows(marker.len()).any(|part| part == *marker));
            if damaged {
                input_damaged.store(true, Ordering::Relaxed);
            }
            // El solape conserva avisos partidos entre lecturas, aunque el log salga de la cola.
            let discard = detection.len().saturating_sub(64);
            detection.drain(..discard);
        }
        let overflow = (retained.len() + read).saturating_sub(MAX_FFMPEG_STDERR_BYTES);
        retained.drain(..overflow);
        retained.extend(&chunk[..read]);
    }

    retained.into_iter().collect()
}

pub(super) async fn run_ffmpeg(
    ffmpeg_path: &Path,
    stream_url: &str,
    output_path: &Path,
    max_duration_secs: Option<u64>,
    min_free_space: u64,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<(), InfrastructureError> {
    let result = run_ffmpeg_once(
        ffmpeg_path,
        stream_url,
        output_path,
        max_duration_secs,
        min_free_space,
        cancel_rx.clone(),
        true,
    )
    .await;
    let unsupported = matches!(&result, Err(InfrastructureError::RecordingError(message)) if message.contains("seg_max_retry") && (message.contains("Unrecognized option") || message.contains("Option not found")));
    if unsupported {
        let empty = match tokio::fs::metadata(output_path).await {
            Ok(metadata) => metadata.len() == 0,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
        if empty {
            // FFmpeg anterior a 6.1 no ofrece este reintento; no reabrir datos ya escritos.
            return run_ffmpeg_once(
                ffmpeg_path,
                stream_url,
                output_path,
                max_duration_secs,
                min_free_space,
                cancel_rx,
                false,
            )
            .await;
        }
    }
    result
}

async fn run_ffmpeg_once(
    ffmpeg_path: &Path,
    stream_url: &str,
    output_path: &Path,
    max_duration_secs: Option<u64>,
    min_free_space: u64,
    cancel_rx: Option<watch::Receiver<bool>>,
    segment_retry: bool,
) -> Result<(), InfrastructureError> {
    let input_damaged = Arc::new(AtomicBool::new(false));
    if cancel_rx
        .as_ref()
        .is_some_and(|receiver| *receiver.borrow())
    {
        return Err(InfrastructureError::RecordingCancelled);
    }

    ensure_disk_space(output_path, min_free_space)?;

    let mut command = tokio::process::Command::new(ffmpeg_path);
    command.kill_on_drop(true);
    configure_process_isolation(&mut command);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("warning")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    command.env_remove("CBREC_SESSION_COOKIE");

    let mut child = command
        .args(duration_args(max_duration_secs))
        .args(input_recovery_args(stream_url, segment_retry))
        .arg("-i")
        .arg(stream_url)
        .arg("-c")
        .arg("copy")
        .args(recording_format_args(output_path))
        .arg("-y")
        .arg(output_path)
        .spawn()
        .map_err(|e| {
            InfrastructureError::RecordingError(format!("Failed to start ffmpeg: {}", e))
        })?;
    // Child::wait cierra stdin; conservarlo permite finalizar con q antes de recurrir a señales.
    let mut stdin = child.stdin.take();
    let mut stderr_task = child.stderr.take().map(|mut stderr| {
        let damaged = input_damaged.clone();
        tokio::spawn(async move { read_bounded_stderr(&mut stderr, &damaged).await })
    });

    if let Some(mut cancel_rx) = cancel_rx {
        if *cancel_rx.borrow() {
            cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
            ensure_input_integrity(&input_damaged)?;
            return Err(InfrastructureError::RecordingCancelled);
        }

        tokio::select! {
            status = child.wait() => {
                check_exit_status(status, stderr_task.take()).await?;
            }
            _ = async { cancel_rx.wait_for(|value| *value).await.ok(); } => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                ensure_input_integrity(&input_damaged)?;
                return Err(InfrastructureError::RecordingCancelled);
            }
            _ = wait_for_duration_limit(max_duration_secs) => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                ensure_input_integrity(&input_damaged)?;
                return Err(InfrastructureError::RecordingCancelled);
            }
            _ = wait_for_stall(
                output_path,
                Duration::from_secs(STALL_TIMEOUT_SECS),
                Duration::from_secs(STALL_CHECK_SECS),
            ) => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                return Err(InfrastructureError::RecordingError(format!(
                    "FFmpeg no escribio datos nuevos durante {} segundos",
                    STALL_TIMEOUT_SECS
                )));
            }
            available = wait_for_low_disk(output_path, min_free_space) => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                return Err(low_disk_error(available?, min_free_space));
            }
        }
    } else {
        tokio::select! {
            status = child.wait() => {
                check_exit_status(status, stderr_task.take()).await?;
            }
            _ = wait_for_duration_limit(max_duration_secs) => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                ensure_input_integrity(&input_damaged)?;
                return Err(InfrastructureError::RecordingCancelled);
            }
            _ = wait_for_stall(
                output_path,
                Duration::from_secs(STALL_TIMEOUT_SECS),
                Duration::from_secs(STALL_CHECK_SECS),
            ) => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                return Err(InfrastructureError::RecordingError(format!(
                    "FFmpeg no escribio datos nuevos durante {} segundos",
                    STALL_TIMEOUT_SECS
                )));
            }
            available = wait_for_low_disk(output_path, min_free_space) => {
                cancel_ffmpeg(&mut child, stdin.take(), stderr_task.take()).await;
                return Err(low_disk_error(available?, min_free_space));
            }
        }
    }

    ensure_input_integrity(&input_damaged)?;
    Ok(())
}

fn ensure_input_integrity(input_damaged: &AtomicBool) -> Result<(), InfrastructureError> {
    if input_damaged.load(Ordering::Relaxed) {
        return Err(InfrastructureError::RecordingError("FFmpeg detectó paquetes corruptos o segmentos perdidos; revisa la integridad de la grabación".into()));
    }
    Ok(())
}

fn input_recovery_args(stream_url: &str, segment_retry: bool) -> &'static [&'static str] {
    if !url::Url::parse(stream_url)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
    {
        return &[];
    }
    // El cliente valida HLS; reintentar segmentos evita saltos y no debe reiniciar su EOF normal.
    let args: &[&str] = &[
        "-rw_timeout",
        "15000000",
        "-reconnect",
        "1",
        "-reconnect_streamed",
        "1",
        "-reconnect_on_network_error",
        "1",
        "-reconnect_on_http_error",
        "429,500,502,503,504",
        "-reconnect_delay_max",
        "5",
        "-seg_max_retry",
        "3",
    ];
    if segment_retry {
        args
    } else {
        &args[..args.len() - 2]
    }
}

fn recording_format_args(path: &Path) -> &'static [&'static str] {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "m4v" | "mov"
            )
        })
    {
        // Los fragmentos limitan la memoria y conservan contenido legible ante un corte.
        &[
            "-movflags",
            "+frag_keyframe+empty_moov+default_base_moof",
            "-frag_duration",
            "5000000",
            "-flush_packets",
            "1",
        ]
    } else {
        &[]
    }
}

fn ensure_disk_space(path: &Path, required: u64) -> Result<(), InfrastructureError> {
    if required == 0 {
        return Ok(());
    }
    let available = available_disk_space(path)?;
    if available < required {
        return Err(low_disk_error(available, required));
    }
    Ok(())
}

async fn wait_for_low_disk(path: &Path, required: u64) -> Result<u64, std::io::Error> {
    if required == 0 {
        return future::pending().await;
    }

    loop {
        tokio::time::sleep(Duration::from_secs(DISK_CHECK_SECS)).await;
        let available = available_disk_space(path)?;
        if available < required {
            return Ok(available);
        }
    }
}

fn available_disk_space(path: &Path) -> Result<u64, std::io::Error> {
    fs4::available_space(path.parent().unwrap_or_else(|| Path::new(".")))
}

fn low_disk_error(available: u64, required: u64) -> InfrastructureError {
    InfrastructureError::RecordingError(format!(
        "espacio insuficiente: {available} bytes disponibles; se requieren {required}"
    ))
}

async fn check_exit_status(
    status: std::io::Result<ExitStatus>,
    stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>,
) -> Result<(), InfrastructureError> {
    let stderr = read_stderr(stderr_task).await;
    let status = status.map_err(|e| {
        InfrastructureError::RecordingError(format!("Failed to wait for ffmpeg: {}", e))
    })?;
    if !status.success() {
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
        Some(mut task) => {
            // Un descendiente puede conservar el pipe abierto aunque FFmpeg ya terminó.
            match tokio::time::timeout(Duration::from_secs(1), &mut task).await {
                Ok(result) => result.unwrap_or_default(),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    Vec::new()
                }
            }
        }
        None => Vec::new(),
    }
}

async fn cancel_ffmpeg(
    child: &mut Child,
    stdin: Option<ChildStdin>,
    stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>,
) {
    if !request_stdin_shutdown(stdin).await {
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

async fn request_stdin_shutdown(stdin: Option<ChildStdin>) -> bool {
    let Some(mut stdin) = stdin else {
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

    // El error final debe aparecer antes de los avisos repetidos de una emisión larga.
    for line in text.lines().rev() {
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
    } else if line.to_ascii_lowercase().contains("http://")
        || line.to_ascii_lowercase().contains("https://")
    {
        "[URL multimedia omitida]".to_string()
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stderr_reader_does_not_outlive_successful_process() {
        let task = tokio::spawn(future::pending::<Vec<u8>>());
        let abort = task.abort_handle();
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        };
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        };
        check_exit_status(Ok(status), Some(task)).await.unwrap();
        assert!(abort.is_finished(), "El lector no debe quedar ejecutandose");
    }

    #[tokio::test]
    async fn stderr_reader_has_a_deadline_and_is_reaped() {
        let task = tokio::spawn(future::pending::<Vec<u8>>());
        let abort = task.abort_handle();
        let result = tokio::time::timeout(Duration::from_secs(3), read_stderr(Some(task))).await;
        assert!(
            result.is_ok(),
            "El cierre no debe esperar indefinidamente EOF"
        );
        assert!(
            abort.is_finished(),
            "El lector debe recogerse al vencer el plazo"
        );
    }

    #[test]
    fn stderr_does_not_expose_media_url_tokens() {
        let summary = summarize_stderr(b"Error al abrir https://media.example/video?token=CANARY_FAKE_TOKEN\nfallo de conexion").unwrap();
        assert!(!summary.contains("CANARY_FAKE_TOKEN"));
        assert!(summary.contains("fallo de conexion"));
    }

    #[tokio::test]
    #[ignore = "Requires FFmpeg and ffprobe; run explicitly in CI"]
    async fn ffmpeg_smoke_records_playable_timed_clip() {
        let root = std::env::temp_dir().join(format!(
            "cbrec_ffmpeg_smoke_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let source = root.join("source.mp4");
        let destination = root.join("clip.mp4");
        let generated = tokio::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:r=10",
                "-t",
                "2",
                "-c:v",
                "mpeg4",
            ])
            .arg(&source)
            .output()
            .await
            .expect("FFmpeg must be installed");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );

        run_ffmpeg(
            Path::new("ffmpeg"),
            source.to_str().unwrap(),
            &destination,
            Some(1),
            0,
            None,
        )
        .await
        .unwrap();
        let probe = tokio::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(&destination)
            .output()
            .await
            .expect("ffprobe must be installed");
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let duration: f64 = String::from_utf8(probe.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            (0.9..=1.2).contains(&duration),
            "Unexpected clip duration: {duration}"
        );

        let bytes = tokio::fs::read(&destination).await.unwrap();
        let mut offset = 0;
        let mut fragment_end = None;
        while offset + 8 <= bytes.len() {
            let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            assert!(size >= 8 && offset + size <= bytes.len());
            if &bytes[offset + 4..offset + 8] == b"mdat" {
                fragment_end = Some(offset + size);
                break;
            }
            offset += size;
        }
        // Simula el archivo persistido hasta el primer fragmento, sin el cierre final.
        let interrupted = root.join("interrupted.mp4");
        tokio::fs::write(
            &interrupted,
            &bytes[..fragment_end.expect("falta contenido multimedia")],
        )
        .await
        .unwrap();
        let decoded = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-xerror", "-i"])
            .arg(&interrupted)
            .args(["-f", "framehash", "-"])
            .output()
            .await
            .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert!(String::from_utf8_lossy(&decoded.stdout)
            .lines()
            .any(|line| !line.starts_with('#') && !line.trim().is_empty()));

        let live = root.join("live.mp4");
        let mut child = tokio::process::Command::new("ffmpeg")
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .args(["-v", "error", "-re", "-stream_loop", "-1", "-i"])
            .arg(&source)
            .args(["-c", "copy"])
            .args(recording_format_args(&live))
            .arg(&live)
            .spawn()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if tokio::fs::metadata(&live)
                    .await
                    .is_ok_and(|metadata| metadata.len() >= fragment_end.unwrap() as u64)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("no se persistieron fragmentos durante la grabacion");
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        let probe = tokio::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_packets",
                "-show_entries",
                "stream=nb_read_packets",
                "-of",
                "csv=p=0",
            ])
            .arg(&live)
            .output()
            .await
            .unwrap();
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        assert!(
            String::from_utf8_lossy(&probe.stdout)
                .trim()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "Requiere FFmpeg y ffprobe; ejecutar explícitamente en CI"]
    async fn transient_hls_segment_failure_keeps_all_video_packets() {
        assert_hls_recording_recovers(false, false).await;
    }

    #[tokio::test]
    #[ignore = "Requiere FFmpeg y ffprobe; ejecutar explícitamente en CI"]
    async fn partial_hls_segment_is_reported_and_preserved() {
        assert_hls_recording_recovers(true, false).await;
    }

    #[tokio::test]
    #[ignore = "Requiere FFmpeg y ffprobe; ejecutar explícitamente en CI"]
    async fn buffered_hls_relay_recovers_partial_segments_without_losing_frames() {
        assert_hls_recording_recovers(true, true).await;
    }

    #[tokio::test]
    #[ignore = "Requiere FFmpeg y ffprobe; ejecutar explícitamente en CI"]
    async fn buffered_hls_relay_retries_http_failures_without_changing_frames() {
        assert_hls_recording_recovers(false, true).await;
    }

    async fn assert_hls_recording_recovers(truncate_segment: bool, buffered: bool) {
        use std::collections::HashMap;
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let root = std::env::temp_dir().join(format!(
            "cbrec_hls_retry_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let source = root.join("source.m3u8");
        let generated = tokio::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x64:rate=10",
                "-t",
                "12",
                "-c:v",
                "mpeg2video",
                "-g",
                "10",
                "-sc_threshold",
                "1000000000",
                "-f",
                "hls",
                "-hls_time",
                "1",
                "-hls_list_size",
                "0",
                "-hls_segment_filename",
            ])
            .arg(root.join("seg%d.ts"))
            .arg(&source)
            .output()
            .await
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let mut assets = HashMap::new();
        let mut files = tokio::fs::read_dir(&root).await.unwrap();
        while let Some(file) = files.next_entry().await.unwrap() {
            assets.insert(
                format!("/{}", file.file_name().to_string_lossy()),
                tokio::fs::read(file.path()).await.unwrap(),
            );
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/source.m3u8", listener.local_addr().unwrap());
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let playlist_attempts = Arc::new(AtomicUsize::new(0));
        let playlist_count = playlist_attempts.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|p| p == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let size = socket.read(&mut buffer).await.unwrap();
                    if size == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..size]);
                    assert!(request.len() < 8192);
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let attempt = if path == "/seg8.ts" {
                    count.fetch_add(1, Ordering::SeqCst)
                } else {
                    usize::MAX
                };
                let failing = !truncate_segment && attempt < 5;
                let truncated = truncate_segment && attempt == 0;
                let master_failed =
                    path == "/source.m3u8" && playlist_count.fetch_add(1, Ordering::SeqCst) == 0;
                let (status, body) = if failing || master_failed {
                    ("503 Service Unavailable", &[][..])
                } else if let Some(body) = assets.get(path) {
                    ("200 OK", body.as_slice())
                } else {
                    ("404 Not Found", &[][..])
                };
                let start = request
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(key, _)| key.eq_ignore_ascii_case("range"))
                    .and_then(|(_, range)| range.trim().strip_prefix("bytes="))
                    .and_then(|range| range.split('-').next())
                    .and_then(|start| start.parse::<usize>().ok())
                    .unwrap_or(0);
                let (status, range) = if status == "200 OK" && start > 0 {
                    (
                        "206 Partial Content",
                        format!(
                            "Content-Range: bytes {start}-{}/{}\r\n",
                            body.len() - 1,
                            body.len()
                        ),
                    )
                } else {
                    (status, String::new())
                };
                let served = &body[start.min(body.len())..];
                let header = format!(
                    "HTTP/1.1 {status}\r\n{range}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    served.len()
                );
                if socket.write_all(header.as_bytes()).await.is_ok() {
                    let bytes = if truncated {
                        &served[..served.len() / 2]
                    } else {
                        served
                    };
                    let _ = socket.write_all(bytes).await;
                }
            }
        });
        let relay = if buffered {
            Some(
                super::super::hls_relay::HlsRelay::start(reqwest::Client::new(), &url)
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        let input = relay
            .as_ref()
            .map_or(url.as_str(), |relay| relay.url.as_str());
        let destination = root.join("recording.mp4");
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            run_ffmpeg(Path::new("ffmpeg"), input, &destination, None, 0, None),
        )
        .await;
        server.abort();
        let result = result.unwrap();
        if truncate_segment && !buffered {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("paquetes corruptos o segmentos perdidos"),
                "{error}"
            );
            assert!(tokio::fs::metadata(&destination).await.unwrap().len() > 0);
            tokio::fs::remove_dir_all(root).await.unwrap();
            return;
        }
        result.unwrap();
        let probe = tokio::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-count_packets",
                "-show_entries",
                "stream=nb_read_packets",
                "-of",
                "json",
            ])
            .arg(&destination)
            .output()
            .await
            .unwrap();
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let packets = report["streams"][0]["nb_read_packets"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert_eq!(
            packets, 120,
            "Un fallo HTTP transitorio no debe descartar un segmento de vídeo"
        );
        assert!(attempts.load(Ordering::SeqCst) >= if truncate_segment { 2 } else { 6 });
        assert!(playlist_attempts.load(Ordering::SeqCst) >= 2);
        let mut hashes = Vec::new();
        for input in [&source, &destination] {
            let decoded = tokio::process::Command::new("ffmpeg")
                .args(["-v", "error", "-xerror", "-i"])
                .arg(input)
                .args(["-map", "0:v:0", "-f", "framemd5", "-"])
                .output()
                .await
                .unwrap();
            assert!(
                decoded.status.success(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            let frames: Vec<String> = String::from_utf8_lossy(&decoded.stdout)
                .lines()
                .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
                .map(|line| line.rsplit(',').next().unwrap().trim().to_string())
                .collect();
            assert_eq!(frames.len(), 120);
            hashes.push(frames);
        }
        assert!(
            hashes[0] == hashes[1],
            "La recuperación debe conservar los píxeles decodificados de todos los cuadros"
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_ffmpeg_falls_back_only_before_any_recording_bytes_exist() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "cbrec_legacy_ffmpeg_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let executable = root.join("ffmpeg");
        tokio::fs::write(
            &executable,
            r#"#!/bin/sh
for arg do
    if [ "$arg" = "-seg_max_retry" ]; then
        echo "Unrecognized option 'seg_max_retry'" >&2
        exit 1
    fi
    output="$arg"
done
printf 'fallback recording' > "$output"
"#,
        )
        .await
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = root.join("recording.mp4");
        run_ffmpeg(
            &executable,
            "https://example.com/live.m3u8",
            &output,
            None,
            0,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::fs::read(&output).await.unwrap(),
            b"fallback recording"
        );
        tokio::fs::write(&output, b"existing bytes must survive")
            .await
            .unwrap();
        assert!(run_ffmpeg(
            &executable,
            "https://example.com/live.m3u8",
            &output,
            None,
            0,
            None
        )
        .await
        .is_err());
        assert_eq!(
            tokio::fs::read(&output).await.unwrap(),
            b"existing bytes must survive"
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn http_recovery_limits_transient_statuses_and_preserves_normal_eof() {
        for source in ["local.mp4", "file:///tmp/local.mp4", "pipe:0"] {
            assert!(input_recovery_args(source, true).is_empty());
        }
        let options = input_recovery_args("https://example.com/live.m3u8", true);
        let get = |key| {
            options
                .as_chunks::<2>()
                .0
                .iter()
                .find(|p| p[0] == key)
                .map(|p| p[1])
        };
        assert_eq!(get("-reconnect_on_http_error"), Some("429,500,502,503,504"));
        assert_eq!(get("-reconnect_delay_max"), Some("5"));
        assert!(!input_recovery_args("https://example.com/live.m3u8", false)
            .contains(&"-seg_max_retry"));
        assert_eq!(get("-rw_timeout"), Some("15000000"));
        assert_eq!(get("-seg_max_retry"), Some("3"));
        assert_eq!(get("-reconnect_at_eof"), None);
    }

    #[test]
    fn fragmentation_only_applies_to_mp4_containers() {
        for extension in ["mp4", "MP4", "mov", "m4v"] {
            assert!(!recording_format_args(Path::new(&format!("video.{extension}"))).is_empty());
        }
        for extension in ["mkv", "ts", "mp3"] {
            assert!(recording_format_args(Path::new(&format!("video.{extension}"))).is_empty());
        }
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

        assert_eq!(summary, "failure | Cookie: [redacted]");
    }

    #[test]
    fn stderr_summary_limits_lines() {
        let stderr = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let summary = summarize_stderr(stderr).unwrap();

        assert_eq!(summary, "9 | 8 | 7 | 6 | 5 | 4 | 3 | 2");
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
    async fn stderr_reader_drains_input_and_keeps_the_final_failure() {
        let mut input = vec![b'x'; MAX_FFMPEG_STDERR_BYTES * 3];
        input.extend_from_slice(b"\nfatal: connection lost at the end\n");
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        let writer_task = tokio::spawn(async move {
            writer.write_all(&input).await.expect("write stderr data");
        });

        let captured = read_bounded_stderr(&mut reader, &AtomicBool::new(false)).await;
        writer_task.await.expect("stderr writer completes");

        assert_eq!(captured.len(), MAX_FFMPEG_STDERR_BYTES);
        assert!(captured.ends_with(b"fatal: connection lost at the end\n"));
        assert!(summarize_stderr(&captured)
            .unwrap()
            .starts_with("fatal: connection lost at the end"));
    }

    #[tokio::test]
    async fn damaged_input_is_remembered_after_the_warning_leaves_the_log_tail() {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        let writer = tokio::spawn(async move {
            writer.write_all(b"Packet corrupt\n").await.unwrap();
            writer
                .write_all(&vec![b'x'; MAX_FFMPEG_STDERR_BYTES * 2])
                .await
                .unwrap();
        });
        let damaged = AtomicBool::new(false);
        let captured = read_bounded_stderr(&mut reader, &damaged).await;
        writer.await.unwrap();
        assert_eq!(captured.len(), MAX_FFMPEG_STDERR_BYTES);
        assert!(captured.iter().all(|byte| *byte == b'x'));
        assert!(ensure_input_integrity(&damaged).is_err());
        assert!(ensure_input_integrity(&AtomicBool::new(false)).is_ok());
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
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "read line; [ \"$line\" = q ]"]);
        command.kill_on_drop(true).stdin(Stdio::piped());
        let mut child = command.spawn().expect("start test process");

        assert!(request_stdin_shutdown(child.stdin.take()).await);
        let status = tokio::time::timeout(Duration::from_millis(500), child.wait())
            .await
            .expect("process must receive stdin command")
            .expect("wait for test process");
        assert!(status.success());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_isolation_creates_a_process_group() {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "read line"]);
        command.kill_on_drop(true).stdin(Stdio::piped());
        configure_process_isolation(&mut command);
        let mut child = command.spawn().expect("start isolated process");
        let pid = child.id().expect("child process id") as i32;

        assert_eq!(unsafe { libc::getpgid(pid) }, pid);

        let _ = child.kill().await;
        let _ = child.wait().await;
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
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", &body]);
        command.kill_on_drop(true).stdin(Stdio::piped());
        let mut child = command.spawn().expect("start test process");

        let stdin = child.stdin.take();
        cancel_ffmpeg(&mut child, stdin, None).await;

        assert!(child.try_wait().expect("inspect child status").is_some());
        assert_eq!(
            tokio::fs::read_to_string(&marker)
                .await
                .expect("read shutdown marker"),
            "q"
        );
        let _ = tokio::fs::remove_file(marker).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn successful_process_returns_ok() {
        let output = std::env::temp_dir().join("cbrec_success.mp4");

        let result = run_ffmpeg(
            Path::new("true"),
            "https://example.com/live.m3u8",
            &output,
            None,
            0,
            None,
        )
        .await;

        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_process_reports_nonzero_exit() {
        let output = std::env::temp_dir().join("cbrec_failure.mp4");

        let error = run_ffmpeg(
            Path::new("false"),
            "https://example.com/live.m3u8",
            &output,
            None,
            0,
            None,
        )
        .await
        .expect_err("non-zero process must fail")
        .to_string();

        assert!(error.contains("FFmpeg exited with status"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_cancelled_process_returns_cancelled() {
        let executable = std::env::temp_dir().join("cbrec_missing_ffmpeg_executable");
        let output = executable.with_extension("mp4");
        let (cancel_tx, cancel_rx) = watch::channel(true);

        let result = run_ffmpeg(
            &executable,
            "https://example.com/live.m3u8",
            &output,
            None,
            0,
            Some(cancel_rx),
        )
        .await;

        assert!(matches!(
            result,
            Err(InfrastructureError::RecordingCancelled)
        ));
        drop(cancel_tx);
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
            0,
            None,
        )
        .await
        .expect_err("missing executable must fail")
        .to_string();

        assert!(error.contains("Failed to start ffmpeg"));
    }

    #[tokio::test]
    async fn insufficient_disk_space_prevents_process_start() {
        let executable = std::env::temp_dir().join("cbrec_missing_ffmpeg_executable");
        let output = executable.with_extension("mp4");

        let error = run_ffmpeg(
            &executable,
            "https://example.com/live.m3u8",
            &output,
            None,
            u64::MAX,
            None,
        )
        .await
        .expect_err("low disk must fail before process start")
        .to_string();

        assert!(error.contains("espacio insuficiente"));
        assert!(!error.contains("Failed to start ffmpeg"));
    }
}
