mod extract;
mod fetch;
mod find;
mod sitemap;
mod stats;

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// HTTP Basic Auth credentials, as USER:PASS
        #[arg(short = 'u', long, value_name = "USER:PASS")]
        user: Option<String>,
        /// Max simultaneous requests; lower this if a site's WAF starts 403ing under load
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
        /// Milliseconds to wait before each request; use to crawl politely / avoid bot detection
        #[arg(long, default_value_t = 0)]
        delay_ms: u64,
    },
    /// Search downloaded sitemap loc entries for a substring
    Find {
        substring: String,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Copy sitemap files out of an extracted AEM content package into ./sitemaps/
    /// (flattened, same layout `fetch` produces; skips JCR metadata files)
    Extract {
        #[arg(default_value = "content-package")]
        source: PathBuf,
    },
    /// Summarize what's downloaded under ./sitemaps/ (sitemap/index counts, loc entries per domain)
    Stats {
        #[arg(long)]
        domain: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Fetch { url, user_agent, user, concurrency, delay_ms } => {
            let basic_auth = user
                .map(|u| {
                    let (user, pass) = u
                        .split_once(':')
                        .with_context(|| format!("--user must be USER:PASS, got '{u}'"))?;
                    anyhow::Ok((user.to_string(), pass.to_string()))
                })
                .transpose()?;
            fetch::run(&url, &user_agent, basic_auth, concurrency, delay_ms).await
        }
        Command::Find { substring, domain } => find::run(&substring, domain.as_deref()),
        Command::Extract { source } => extract::run(&source),
        Command::Stats { domain } => stats::run(domain.as_deref()),
    }
}
