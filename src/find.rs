use crate::sitemap::{self, SitemapKind};
use anyhow::{bail, Result};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

pub fn run(substring: &str, domain: Option<&str>) -> Result<()> {
    let start = Instant::now();
    let base = PathBuf::from("sitemaps");

    let domain_dirs: Vec<PathBuf> = match domain {
        Some(d) => {
            let dir = base.join(d);
            if !dir.is_dir() {
                bail!("no sitemaps found for domain '{d}' (expected directory {})", dir.display());
            }
            vec![dir]
        }
        None => std::fs::read_dir(&base)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default(),
    };

    let mut matches = Vec::new();
    let mut sitemaps_scanned = 0usize;
    let mut loc_entries_scanned = 0usize;

    for dir in &domain_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(SitemapKind::UrlSet(locs)) = sitemap::parse(&bytes) else { continue };

            sitemaps_scanned += 1;
            loc_entries_scanned += locs.len();
            matches.extend(locs.into_iter().filter(|loc| loc.contains(substring)));
        }
    }

    let matches_found = matches.len();
    let output = json!({
        "matches": matches,
        "stats": {
            "duration_ms": start.elapsed().as_millis(),
            "sitemaps_scanned": sitemaps_scanned,
            "loc_entries_scanned": loc_entries_scanned,
            "matches_found": matches_found,
        }
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
