use cbrec::application::ejecutar_cli;
use cbrec::infrastructure::{AppConfig, ChaturbateClient};
use cbrec::presentation::Cli;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load();
    let client = ChaturbateClient::new()?;

    ejecutar_cli(cli, config, client).await?;

    Ok(())
}
