use cbrec::application::ejecutar_cli;
use cbrec::infrastructure::{AppConfig, ChaturbateClient};
use cbrec::presentation::Cli;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let loaded_config = AppConfig::load_with_warnings();
    let client = ChaturbateClient::new()?;

    ejecutar_cli(cli, loaded_config.config, loaded_config.warnings, client).await?;

    Ok(())
}
