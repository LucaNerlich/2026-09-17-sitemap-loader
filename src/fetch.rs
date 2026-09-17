use crate::sitemap::{self, SitemapKind};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

const MAX_CONCURRENT_FETCHES: usize = 8;

#[derive(Default)]
struct Stats {
    sitemaps_downloaded: usize,
    sitemaps_failed: usize,
    loc_entries_found: usize,
}

pub async fn run(url: &str, user_agent: &str, basic_auth: Option<(String, String)>) -> Result<()> {
    let start = Instant::now();
    let host = url::Url::parse(url)
        .context("invalid URL")?
        .host_str()
        .context("URL has no host")?
        .to_string();

    let dir = PathBuf::from("sitemaps").join(&host);
    std::fs::create_dir_all(&dir)?;

    let client = reqwest::Client::builder().user_agent(user_agent).build()?;
    let basic_auth = Arc::new(basic_auth);
    let stats = Arc::new(Mutex::new(Stats::default()));
    let used_names = Arc::new(Mutex::new(HashSet::new()));
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));

    let mut join_set = JoinSet::new();
    join_set.spawn(fetch_one(
        client.clone(),
        url.to_string(),
        dir.clone(),
        basic_auth.clone(),
        stats.clone(),
        used_names.clone(),
        semaphore.clone(),
    ));

    while let Some(res) = join_set.join_next().await {
        if let Ok(children) = res {
            for child in children {
                join_set.spawn(fetch_one(
                    client.clone(),
                    child,
                    dir.clone(),
                    basic_auth.clone(),
                    stats.clone(),
                    used_names.clone(),
                    semaphore.clone(),
                ));
            }
        }
    }

    let stats = stats.lock().await;
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
async fn fetch_one(
    client: reqwest::Client,
    url: String,
    dir: PathBuf,
    basic_auth: Arc<Option<(String, String)>>,
    stats: Arc<Mutex<Stats>>,
    used_names: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
) -> Vec<String> {
    let _permit = semaphore.acquire_owned().await.expect("semaphore never closed");

    let outcome: Result<SitemapKind> = async {
        let mut req = client.get(&url);
        if let Some((user, pass)) = basic_auth.as_ref() {
            req = req.basic_auth(user, Some(pass));
        }
        let bytes = req.send().await?.error_for_status()?.bytes().await?;
        let bytes = sitemap::maybe_gunzip(bytes.to_vec());
        let kind = sitemap::parse(&bytes)?;
        let name = unique_name(&url, &used_names).await;
        std::fs::write(dir.join(&name), &bytes)?;
        Ok(kind)
    }
    .await;

    match outcome {
        Ok(SitemapKind::Index(children)) => {
            let mut s = stats.lock().await;
            s.sitemaps_downloaded += 1;
            eprintln!("[{}] index   {url} ({} child sitemaps)", s.sitemaps_downloaded, children.len());
            children
        }
        Ok(SitemapKind::UrlSet(locs)) => {
            let mut s = stats.lock().await;
            s.sitemaps_downloaded += 1;
            s.loc_entries_found += locs.len();
            eprintln!("[{}] sitemap {url} ({} locs)", s.sitemaps_downloaded, locs.len());
            Vec::new()
        }
        Err(e) => {
            let mut s = stats.lock().await;
            s.sitemaps_failed += 1;
            eprintln!("[{}] failed  {url}: {e}", s.sitemaps_downloaded + s.sitemaps_failed);
            Vec::new()
        }
    }
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
