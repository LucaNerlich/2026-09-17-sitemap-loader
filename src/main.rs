mod fetch;
mod find;
mod sitemap;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sitemapper", about = "Download and search XML sitemaps")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download a sitemap (or sitemap index) into ./sitemaps/<domain>/
    Fetch {
        url: String,
        /// User-Agent header to send; some sites reject requests with no/generic UA
        #[arg(long, default_value = concat!("sitemapper/", env!("CARGO_PKG_VERSION")))]
        user_agent: String,
    },
    /// Search downloaded sitemap loc entries for a substring
    Find {
        substring: String,
        #[arg(long)]
        domain: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Fetch { url, user_agent } => fetch::run(&url, &user_agent).await,
        Command::Find { substring, domain } => find::run(&substring, domain.as_deref()),
    }
}
