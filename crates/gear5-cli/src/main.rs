mod cmd;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "gear5", version, about = "gear5-rs admin CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Apply pending database migrations.
    Migrate,
    /// Scrape management.
    #[command(subcommand)]
    Scrape(cmd::scrape::ScrapeCmd),
    /// API key management.
    #[command(subcommand)]
    Key(cmd::key::KeyCmd),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Migrate => cmd::migrate::run().await,
        Command::Scrape(s) => cmd::scrape::run(s).await,
        Command::Key(k) => cmd::key::run(k).await,
    }
}
