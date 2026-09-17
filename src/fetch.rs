use crate::sitemap::{self, SitemapKind};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

#[derive(Default)]
struct Stats {
    sitemaps_downloaded: usize,
    sitemaps_failed: usize,
    loc_entries_found: usize,
}

#[derive(Clone)]
struct FetchCtx {
    client: reqwest::Client,
    dir: PathBuf,
    basic_auth: Arc<Option<(String, String)>>,
    delay: Duration,
    stats: Arc<Mutex<Stats>>,
    used_names: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
}

pub async fn run(
    url: &str,
    user_agent: &str,
    basic_auth: Option<(String, String)>,
    concurrency: usize,
    delay_ms: u64,
) -> Result<()> {
    let start = Instant::now();
    let parsed_url = url::Url::parse(url).context("invalid URL")?;
    let host = parsed_url.host_str().context("URL has no host")?.to_string();

    let dir = PathBuf::from("sitemaps").join(&host);
    std::fs::create_dir_all(&dir)?;

    let ctx = FetchCtx {
        client: reqwest::Client::builder().user_agent(user_agent).build()?,
        dir: dir.clone(),
        basic_auth: Arc::new(basic_auth),
        delay: Duration::from_millis(delay_ms),
        stats: Arc::new(Mutex::new(Stats::default())),
        used_names: Arc::new(Mutex::new(HashSet::new())),
        semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
    };

    // No path (just a bare domain) means "discover sitemaps for me" rather
    // than "this is the sitemap"; robots.txt is the standard place sites
    // declare where their sitemap(s) live.
    let seed_urls = if parsed_url.path() == "/" || parsed_url.path().is_empty() {
        discover_sitemaps_from_robots(&ctx, &parsed_url).await?
    } else {
        vec![url.to_string()]
    };

    let mut join_set = JoinSet::new();
    for seed in seed_urls {
        join_set.spawn(fetch_one(ctx.clone(), seed));
    }

    while let Some(res) = join_set.join_next().await {
        if let Ok(children) = res {
            for child in children {
                join_set.spawn(fetch_one(ctx.clone(), child));
            }
        }
    }

    let stats = ctx.stats.lock().await;
    println!("Fetched sitemap tree for {host}");
    println!("  duration:            {:.2}s", start.elapsed().as_secs_f64());
    println!("  sitemaps downloaded: {}", stats.sitemaps_downloaded);
    println!("  sitemaps failed:     {}", stats.sitemaps_failed);
    println!("  loc entries found:   {}", stats.loc_entries_found);
    println!("  saved to:            {}", dir.display());

    Ok(())
}

/// Fetches and saves a single sitemap file. Never returns Err for network/parse
/// failures (those are recorded in `stats` instead); returns any child sitemap
/// URLs to fetch next if this was a sitemap index.
async fn fetch_one(ctx: FetchCtx, url: String) -> Vec<String> {
    let _permit = ctx.semaphore.acquire_owned().await.expect("semaphore never closed");
    if !ctx.delay.is_zero() {
        tokio::time::sleep(ctx.delay).await;
    }

    let outcome: Result<sitemap::ParsedSitemap> = async {
        let mut req = ctx.client.get(&url);
        if let Some((user, pass)) = ctx.basic_auth.as_ref() {
            req = req.basic_auth(user, Some(pass));
        }
        let bytes = req.send().await?.error_for_status()?.bytes().await?;
        let bytes = sitemap::maybe_gunzip(bytes.to_vec());
        let parsed = sitemap::parse(&bytes)?;
        let name = unique_name(&url, &ctx.used_names).await;
        std::fs::write(ctx.dir.join(&name), &bytes)?;
        Ok(parsed)
    }
    .await;

    match outcome {
        Ok(sitemap::ParsedSitemap { kind: SitemapKind::Index(children), .. }) => {
            let mut s = ctx.stats.lock().await;
            s.sitemaps_downloaded += 1;
            eprintln!("[{}] index   {url} ({} child sitemaps)", s.sitemaps_downloaded, children.len());
            children
        }
        Ok(sitemap::ParsedSitemap { kind: SitemapKind::UrlSet(locs), .. }) => {
            let mut s = ctx.stats.lock().await;
            s.sitemaps_downloaded += 1;
            s.loc_entries_found += locs.len();
            eprintln!("[{}] sitemap {url} ({} locs)", s.sitemaps_downloaded, locs.len());
            Vec::new()
        }
        Err(e) => {
            let mut s = ctx.stats.lock().await;
            s.sitemaps_failed += 1;
            eprintln!("[{}] failed  {url}: {e}", s.sitemaps_downloaded + s.sitemaps_failed);
            Vec::new()
        }
    }
}

async fn discover_sitemaps_from_robots(ctx: &FetchCtx, base_url: &url::Url) -> Result<Vec<String>> {
    let robots_url = format!("{}://{}/robots.txt", base_url.scheme(), base_url.host_str().unwrap());
    eprintln!("no sitemap path given; checking {robots_url}");

    let mut req = ctx.client.get(&robots_url);
    if let Some((user, pass)) = ctx.basic_auth.as_ref() {
        req = req.basic_auth(user, Some(pass));
    }
    let body = req.send().await?.error_for_status()?.text().await?;

    let sitemaps = parse_robots_sitemaps(&body);
    if sitemaps.is_empty() {
        bail!("no 'Sitemap:' entries found in {robots_url}");
    }
    eprintln!("found {} sitemap reference(s) in robots.txt", sitemaps.len());
    Ok(sitemaps)
}

/// Extracts `Sitemap: <url>` directive values from a robots.txt body. The
/// field name is case-insensitive per convention, and sites commonly declare
/// several (see e.g. https://www.sap.com/robots.txt).
fn parse_robots_sitemaps(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let (field, value) = line.trim().split_once(':')?;
            field.eq_ignore_ascii_case("sitemap").then(|| value.trim().to_string())
        })
        .collect()
}

async fn unique_name(url: &str, used: &Arc<Mutex<HashSet<String>>>) -> String {
    let base = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(str::to_string)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "sitemap.xml".to_string());
    let base = sitemap::sanitize_filename(&base);

    let mut used = used.lock().await;
    if used.insert(base.clone()) {
        return base;
    }
    let mut i = 1;
    loop {
        let candidate = format!("{i}-{base}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sitemap_directives_ignoring_case_comments_and_other_fields() {
        let robots = "User-agent: *\n\
             Disallow: /admin\n\
             # Sitemap: https://example.com/should-be-ignored.xml\n\
             Sitemap: https://example.com/sitemap_index.xml\n\
             sitemap: https://example.com/sitemap-index.xml  \n\
             Sitemap:https://example.com/no-space.xml\n";

        assert_eq!(
            parse_robots_sitemaps(robots),
            vec![
                "https://example.com/sitemap_index.xml",
                "https://example.com/sitemap-index.xml",
                "https://example.com/no-space.xml",
            ]
        );
    }

    #[test]
    fn no_sitemap_directives_returns_empty() {
        assert!(parse_robots_sitemaps("User-agent: *\nDisallow: /\n").is_empty());
    }
}
