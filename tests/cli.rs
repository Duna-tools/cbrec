use cbrec::presentation::{Cli, Commands};
use clap::Parser;

#[test]
fn parse_multiple_models_default_jobs() {
    let cli = Cli::parse_from(["cbrec", "alice", "bob"]);
    assert_eq!(cli.modelos, vec!["alice", "bob"]);
    assert_eq!(cli.jobs, None);
    assert!(cli.command.is_none());
}

#[test]
fn parse_jobs_override_main() {
    let cli = Cli::parse_from(["cbrec", "--jobs", "5", "alice"]);
    assert_eq!(cli.modelos, vec!["alice"]);
    assert_eq!(cli.jobs, Some(5));
}

#[test]
fn parse_duration_override_main() {
    let cli = Cli::parse_from(["cbrec", "--duration", "20", "alice"]);
    assert_eq!(cli.modelos, vec!["alice"]);
    assert_eq!(cli.duration, Some(20));
}

#[test]
fn parse_record_with_jobs() {
    let cli = Cli::parse_from(["cbrec", "record", "alice", "bob", "--jobs", "4"]);
    assert_eq!(cli.jobs, Some(4));
    match cli.command {
        Some(Commands::Record { modelos, .. }) => {
            assert_eq!(modelos, vec!["alice", "bob"]);
        }
        _ => panic!("Se esperaba subcomando record"),
    }
}

#[test]
fn parse_check() {
    let cli = Cli::parse_from(["cbrec", "check", "alice"]);
    match cli.command {
        Some(Commands::Check { model, json }) => {
            assert_eq!(model, "alice");
            assert!(!json);
        }
        _ => panic!("Se esperaba subcomando check"),
    }
}

#[test]
fn parse_doctor_command() {
    let cli = Cli::parse_from(["cbrec", "doctor"]);
    assert!(matches!(cli.command, Some(Commands::Doctor)));
}

#[test]
fn parse_discover_command() {
    let cli = Cli::parse_from(["cbrec", "discover", "--tag", "gaming", "--limit", "5"]);
    match cli.command {
        Some(Commands::Discover { tag, limit, json }) => {
            assert_eq!(tag, "gaming");
            assert_eq!(limit, 5);
            assert!(!json);
        }
        _ => panic!("Se esperaba subcomando discover"),
    }
}

#[test]
fn parse_json_query_flags() {
    let check = Cli::parse_from(["cbrec", "check", "alice", "--json"]);
    assert!(matches!(
        check.command,
        Some(Commands::Check { json: true, .. })
    ));

    let discover = Cli::parse_from(["cbrec", "discover", "--tag", "gaming", "--json"]);
    assert!(matches!(
        discover.command,
        Some(Commands::Discover { json: true, .. })
    ));
}

#[test]
fn parse_tui_command() {
    let cli = Cli::parse_from(["cbrec", "tui", "--tag", "gaming", "--limit", "10"]);
    match cli.command {
        Some(Commands::Tui { tag, limit }) => {
            assert_eq!(tag, "gaming");
            assert_eq!(limit, 10);
        }
        _ => panic!("Se esperaba subcomando tui"),
    }
}

#[test]
fn parse_ffmpeg_path_global() {
    let cli = Cli::parse_from(["cbrec", "record", "alice", "--ffmpeg-path", "/tmp/ffmpeg"]);
    assert_eq!(cli.ffmpeg_path.as_deref(), Some("/tmp/ffmpeg"));
}

#[test]
fn parse_list_flag() {
    let cli = Cli::parse_from(["cbrec", "alice", "-l"]);
    assert!(cli.listar);
}

#[test]
fn parse_check_flag() {
    let cli = Cli::parse_from(["cbrec", "alice", "-c"]);
    assert!(cli.verificar);
}

#[test]
fn parse_watch_command() {
    let cli = Cli::parse_from(["cbrec", "watch", "alice", "bob", "-q", "720p"]);
    match cli.command {
        Some(Commands::Watch {
            modelos,
            ask,
            quality,
            ..
        }) => {
            assert_eq!(modelos, vec!["alice", "bob"]);
            assert!(!ask);
            assert_eq!(quality, "720p");
        }
        _ => panic!("Se esperaba subcomando watch"),
    }
}

#[test]
fn parse_watch_ask_flag() {
    let cli = Cli::parse_from(["cbrec", "watch", "alice", "--ask"]);
    match cli.command {
        Some(Commands::Watch { ask, .. }) => assert!(ask),
        _ => panic!("Se esperaba subcomando watch"),
    }
}

#[test]
fn parse_watch_with_jobs() {
    let cli = Cli::parse_from(["cbrec", "watch", "alice", "--jobs", "5"]);
    assert_eq!(cli.jobs, Some(5));
    match cli.command {
        Some(Commands::Watch { modelos, .. }) => assert_eq!(modelos, vec!["alice"]),
        _ => panic!("Se esperaba subcomando watch"),
    }
}

#[test]
fn parse_watch_timeout() {
    let cli = Cli::parse_from(["cbrec", "watch", "alice", "--ask", "--timeout", "30"]);
    match cli.command {
        Some(Commands::Watch { timeout, .. }) => assert_eq!(timeout, Some(30)),
        _ => panic!("Se esperaba subcomando watch"),
    }
}

#[test]
fn parse_watch_no_models() {
    let cli = Cli::parse_from(["cbrec", "watch"]);
    match cli.command {
        Some(Commands::Watch { modelos, .. }) => assert!(modelos.is_empty()),
        _ => panic!("Se esperaba subcomando watch"),
    }
}

#[test]
fn parse_add_command() {
    let cli = Cli::parse_from(["cbrec", "add", "alice", "bob"]);
    match cli.command {
        Some(Commands::Add { models }) => assert_eq!(models, vec!["alice", "bob"]),
        _ => panic!("Se esperaba subcomando add"),
    }
}

#[test]
fn parse_add_url() {
    let cli = Cli::parse_from(["cbrec", "add", "https://chaturbate.com/alice/"]);
    match cli.command {
        Some(Commands::Add { models }) => {
            assert_eq!(models, vec!["https://chaturbate.com/alice/"])
        }
        _ => panic!("Se esperaba subcomando add"),
    }
}

#[test]
fn parse_remove_command() {
    let cli = Cli::parse_from(["cbrec", "remove", "alice"]);
    match cli.command {
        Some(Commands::Remove { models }) => assert_eq!(models, vec!["alice"]),
        _ => panic!("Se esperaba subcomando remove"),
    }
}
