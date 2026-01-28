use cbrec::presentation::{Cli, Commands};
use clap::Parser;

#[test]
fn parse_multiple_models_default_jobs() {
    let cli = Cli::parse_from(["cbrec", "alice", "bob"]);
    assert_eq!(cli.modelos, vec!["alice", "bob"]);
    assert_eq!(cli.jobs, 3);
    assert!(cli.command.is_none());
}

#[test]
fn parse_jobs_override_main() {
    let cli = Cli::parse_from(["cbrec", "--jobs", "5", "alice"]);
    assert_eq!(cli.modelos, vec!["alice"]);
    assert_eq!(cli.jobs, 5);
}

#[test]
fn parse_record_with_jobs() {
    let cli = Cli::parse_from(["cbrec", "record", "alice", "bob", "--jobs", "4"]);
    assert_eq!(cli.jobs, 4);
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
        Some(Commands::Check { model }) => assert_eq!(model, "alice"),
        _ => panic!("Se esperaba subcomando check"),
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
