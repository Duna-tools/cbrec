use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestEnvironment {
    config_home: PathBuf,
    output_root: PathBuf,
    invalid_ffmpeg: PathBuf,
}

impl TestEnvironment {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("reloj del sistema valido")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "cbrec_cli_behavior_{label}_{}_{}",
            std::process::id(),
            nanos
        ));
        let config_home = root.join("config");
        let output_root = root.join("output");
        let invalid_ffmpeg = root.join("missing-ffmpeg");
        fs::create_dir_all(&config_home).expect("crea config temporal");

        Self {
            config_home,
            output_root,
            invalid_ffmpeg,
        }
    }

    fn run(&self, args: &[&str], environment: &[(&str, &Path)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cbrec"));
        command
            .env("CBREC_CONFIG_DIR", self.config_home.join("cbrec"))
            .env_remove("CBREC_SESSION_COOKIE")
            .env_remove("CBREC_FFMPEG")
            .args(args);
        for (name, value) in environment {
            command.env(name, value);
        }
        command.output().expect("ejecuta cbrec")
    }

    fn output_root_arg(&self) -> &str {
        self.output_root
            .to_str()
            .expect("ruta temporal representable")
    }

    fn invalid_ffmpeg_arg(&self) -> &str {
        self.invalid_ffmpeg
            .to_str()
            .expect("ruta temporal representable")
    }

    fn watched_path(&self) -> PathBuf {
        self.config_home.join("cbrec").join("watched.toml")
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let root = self
            .config_home
            .parent()
            .expect("config temporal con directorio padre");
        let _ = fs::remove_dir_all(root);
    }
}

fn assert_command_failed(output: &Output) {
    assert!(
        !output.status.success(),
        "la ejecución debía fallar por la ruta de ffmpeg de prueba"
    );
}

#[test]
fn query_flags_do_not_require_ffmpeg() {
    let environment = TestEnvironment::new("query-without-ffmpeg");
    for flag in ["--check", "--list"] {
        let output = environment.run(
            &[
                "--ffmpeg-path",
                environment.invalid_ffmpeg_arg(),
                flag,
                "invalid model",
            ],
            &[],
        );
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Invalid model name"), "{stderr}");
        assert!(!stderr.contains("Ruta de ffmpeg invalida"));
    }
}

#[test]
fn doctor_reports_effective_environment_session() {
    let environment = TestEnvironment::new("doctor-auth");
    let output = environment.run(
        &[
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "doctor",
        ],
        &[("CBREC_SESSION_COOKIE", Path::new("sessionid=synthetic"))],
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("cookie configurada"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("sin cookie"));
}

#[test]
fn cli_warns_when_jobs_exceed_default_limit() {
    let environment = TestEnvironment::new("jobs-above-limit");
    let output = environment.run(
        &[
            "--jobs",
            "4",
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "doctor",
        ],
        &[],
    );

    assert_command_failed(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("El limite recomendado es 3. Se solicito 4"));
}

#[test]
fn cli_does_not_warn_at_default_jobs_limit() {
    let environment = TestEnvironment::new("jobs-at-limit");
    let output = environment.run(
        &[
            "--jobs",
            "3",
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "doctor",
        ],
        &[],
    );

    assert_command_failed(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("El limite recomendado"));
}

#[test]
fn cli_treats_explicit_ffmpeg_path_as_required() {
    let environment = TestEnvironment::new("explicit-ffmpeg");
    let output = environment.run(
        &[
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "doctor",
        ],
        &[],
    );

    assert_command_failed(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Ruta de ffmpeg invalida"));
}

#[test]
fn cli_treats_non_empty_ffmpeg_environment_as_explicit() {
    let environment = TestEnvironment::new("environment-ffmpeg");
    let output = environment.run(
        &["--output", environment.output_root_arg(), "doctor"],
        &[("CBREC_FFMPEG", &environment.invalid_ffmpeg)],
    );

    assert_command_failed(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Ruta de ffmpeg invalida"));
}

#[test]
fn cli_forwards_configuration_warnings() {
    let environment = TestEnvironment::new("config-warning");
    let config_dir = environment.config_home.join("cbrec");
    fs::create_dir_all(&config_dir).expect("crea directorio de configuracion");
    fs::write(config_dir.join("config.toml"), "[").expect("escribe configuracion invalida");

    let output = environment.run(
        &[
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "doctor",
        ],
        &[],
    );

    assert_command_failed(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config.toml invalido"));
}

#[test]
fn cli_persists_unique_watch_models_before_ffmpeg_validation() {
    let environment = TestEnvironment::new("watch-persistence");
    let output = environment.run(
        &[
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "watch",
            "alice",
            "bob",
        ],
        &[],
    );

    assert_command_failed(&output);
    let watched = fs::read_to_string(environment.watched_path())
        .expect("watchlist guardada antes de validar ffmpeg");
    assert!(watched.contains("alice"));
    assert!(watched.contains("bob"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("modelo(s) duplicados"));
}

#[test]
fn cli_reports_duplicate_watch_models() {
    let environment = TestEnvironment::new("watch-duplicates");
    let output = environment.run(
        &[
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            environment.invalid_ffmpeg_arg(),
            "watch",
            "alice",
            "alice",
            "alice",
        ],
        &[],
    );

    assert_command_failed(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Se omitieron 2 modelo(s) duplicados"));
    assert!(fs::read_to_string(environment.watched_path())
        .expect("watchlist guardada")
        .contains("alice"));
}

#[test]
fn watchlist_read_failure_preserves_original_bytes() {
    let environment = TestEnvironment::new("watchlist-invalid-utf8");
    let path = environment.watched_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = b"models = [\"previous_model\"]\n\xff";
    fs::write(&path, original).unwrap();
    let output = environment.run(&["add", "alice"], &[]);
    assert!(!output.status.success());
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Añadido"));
}

#[test]
fn configuration_errors_do_not_expose_session_values() {
    for content in [
        "[auth]\nsession_cookie = \"PHPSESSID=CANARY_FAKE_COOKIE\" unexpected\n",
        "[general]\nmin_file_size = \"CANARY_FAKE_COOKIE\"\n",
    ] {
        let environment = TestEnvironment::new("config-redaction");
        let config = environment.config_home.join("cbrec");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), content).unwrap();
        let output = environment.run(&["add", "alice"], &[]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("config.toml"));
        assert!(!stderr.contains("CANARY_FAKE_COOKIE"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("CANARY_FAKE_COOKIE"));
    }
}

#[test]
fn invalid_duration_does_not_open_browser_session_receiver() {
    for args in [
        vec!["--browser-session", "--duration", "0", "check", "alice"],
        vec!["--browser-session", "--jobs", "0", "alice"],
        vec!["--browser-session", "bad model"],
        vec!["--browser-session", "--quality", "invalid", "alice"],
        vec!["--browser-session", "watch"],
        vec!["--browser-session", "tui", "--limit", "0"],
        vec!["--browser-session", "tui", "--tag", "bad tag"],
        vec!["--browser-session", "--quality", "invalid", "tui"],
    ] {
        let environment = TestEnvironment::new("validate-before-session");
        let mut child = Command::new(env!("CARGO_BIN_EXE_cbrec"))
            .env("CBREC_CONFIG_DIR", environment.config_home.join("cbrec"))
            .env_remove("CBREC_SESSION_COOKIE")
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let finished = loop {
            if child.try_wait().unwrap().is_some() {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        if !finished {
            child.kill().unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(finished, "la entrada invalida no debe esperar la sesion");
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("http://127.0.0.1:"));
    }
}

#[cfg(unix)]
#[test]
fn doctor_probe_drains_both_pipes_and_omits_session_environment() {
    use std::os::unix::fs::PermissionsExt;
    let environment = TestEnvironment::new("probe-pipes");
    let executable = environment.config_home.join("ffmpeg");
    fs::write(&executable, "#!/bin/sh\nprintf 'ffmpeg version %s\\n' \"${CBREC_SESSION_COOKIE-ausente}\"\ni=0\nwhile [ \"$i\" -lt 6000 ]; do\nprintf 'salida de prueba suficientemente larga\\n'\nprintf 'diagnostico suficientemente largo\\n' >&2\ni=$((i+1))\ndone\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let output = environment.run(
        &[
            "--output",
            environment.output_root_arg(),
            "--ffmpeg-path",
            executable.to_str().unwrap(),
            "doctor",
        ],
        &[("CBREC_SESSION_COOKIE", Path::new("CANARY_FAKE_SESSION"))],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ffmpeg version ausente"));
    assert!(!stdout.contains("CANARY_FAKE_SESSION"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("CANARY_FAKE_SESSION"));
}

#[test]
fn browser_session_cli_accepts_one_session_and_bounds_invalid_attempts() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};
    struct ChildGuard(Option<Child>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    for valid in [true, false] {
        let environment = TestEnvironment::new("browser-bridge");
        let mut child = ChildGuard(Some(
            Command::new(env!("CARGO_BIN_EXE_cbrec"))
                .env("CBREC_CONFIG_DIR", environment.config_home.join("cbrec"))
                .env_remove("CBREC_SESSION_COOKIE")
                .env_remove("CBREC_FFMPEG")
                .args([
                    "--browser-session",
                    "--ffmpeg-path",
                    environment.invalid_ffmpeg_arg(),
                    "record",
                    "alice",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
        let stderr = child.0.as_mut().unwrap().stderr.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut output = String::new();
            for line in BufReader::new(stderr).lines() {
                let line = line.unwrap();
                if line.starts_with("http://127.0.0.1:") {
                    let _ = sender.send(line.clone());
                }
                output.push_str(&line);
                output.push('\n');
            }
            output
        });
        let endpoint = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("El receptor debe anunciar su URL temporal");
        let endpoint = url::Url::parse(&endpoint).unwrap();
        assert_eq!(
            endpoint
                .path()
                .strip_prefix("/cbrec-session/")
                .unwrap()
                .len(),
            32
        );
        for _ in 0..if valid { 1 } else { 3 } {
            let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port().unwrap())).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let body = r#"{"session_cookie":"sessionid=CANARY_SYNTHETIC_BROWSER_SESSION"}"#;
            let path = if valid {
                endpoint.path()
            } else {
                "/incorrect-code"
            };
            write!(
                stream,
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with(if valid {
                "HTTP/1.1 204"
            } else {
                "HTTP/1.1 400"
            }));
            assert!(!response.contains("CANARY_SYNTHETIC_BROWSER_SESSION"));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "El receptor no debe esperar otra sesión"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.0.take().unwrap().wait_with_output().unwrap();
        let stderr = reader.join().unwrap();
        assert!(!output.status.success());
        assert!(stderr.contains(if valid {
            "Sesión recibida"
        } else {
            "demasiadas solicitudes"
        }));
        assert!(!stderr.contains("CANARY_SYNTHETIC_BROWSER_SESSION"));
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("CANARY_SYNTHETIC_BROWSER_SESSION")
        );
        assert!(!environment.watched_path().exists());
    }
}

#[test]
fn tui_rejects_noninteractive_output_before_network_queries() {
    let environment = TestEnvironment::new("tui-terminal");
    let output = environment.run(&["tui", "--tag", "gaming"], &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("terminal interactiva"));
}

#[cfg(unix)]
#[test]
fn tui_home_keyboard_resize_and_terminal_restoration_in_pty() {
    let output = Command::new("python3")
        .args([
            "-B",
            concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/test_tui_pty.py"),
            env!("CARGO_BIN_EXE_cbrec"),
        ])
        .output()
        .expect("Python 3 debe estar disponible para la QA de terminal");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
