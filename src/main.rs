use cbrec::application::ejecutar_cli;
use cbrec::infrastructure::{AppConfig, ChaturbateClient, InfrastructureError};
use cbrec::presentation::Cli;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load();
    let result: Result<ChaturbateClient, InfrastructureError> = ChaturbateClient::new();
    let client = match result {
        Ok(client) => client,
        Err(e) => anyhow::bail!("Failed to initialize Chaturbate client: {}", e),
    };

    ejecutar_cli(cli, config, client).await?;

    Ok(())
}
