use crate::sitemap::{self, SitemapKind};
use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

/// Summarizes what's already downloaded under `./sitemaps/`, per domain: how
/// many sitemap files, how many are indexes vs. urlsets, and total loc entries.
pub fn run(domain: Option<&str>) -> Result<()> {
    let start = Instant::now();
    let domain_dirs = sitemap::resolve_domain_dirs(&PathBuf::from("sitemaps"), domain)?;

    let mut domains = Vec::new();
    let mut total_sitemaps = 0usize;
    let mut total_indexes = 0usize;
    let mut total_locs = 0usize;
    let mut total_xhtml_links = 0usize;
    let mut total_bytes = 0u64;
    let mut total_skipped = 0usize;

    for dir in &domain_dirs {
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut sitemaps = 0usize;
        let mut indexes = 0usize;
        let mut locs = 0usize;
        let mut xhtml_links = 0usize;
        let mut bytes_on_disk = 0u64;
        let mut skipped = 0usize;

        for path in sitemap::find_xml_files(dir) {
            let Ok(bytes) = std::fs::read(&path) else {
                skipped += 1;
                continue;
            };
            match sitemap::parse(&bytes) {
                Ok(sitemap::ParsedSitemap { kind: SitemapKind::UrlSet(l), xhtml_links: links }) => {
                    sitemaps += 1;
                    locs += l.len();
                    xhtml_links += links;
                    bytes_on_disk += bytes.len() as u64;
                }
                Ok(sitemap::ParsedSitemap { kind: SitemapKind::Index(_), .. }) => {
                    indexes += 1;
                    bytes_on_disk += bytes.len() as u64;
                }
                Err(_) => skipped += 1,
            }
        }

        domains.push(json!({
            "domain": name,
            "sitemaps": sitemaps,
            "indexes": indexes,
            "loc_entries": locs,
            "xhtml_links": xhtml_links,
            "xhtml_links_per_loc": xhtml_links_per_loc(xhtml_links, locs),
            "size_bytes": bytes_on_disk,
            "size_human": human_size(bytes_on_disk),
        }));

        total_sitemaps += sitemaps;
        total_indexes += indexes;
        total_locs += locs;
        total_xhtml_links += xhtml_links;
        total_bytes += bytes_on_disk;
        total_skipped += skipped;
    }

    let output = json!({
        "domains": domains,
        "stats": {
            "duration_ms": start.elapsed().as_millis(),
            "domains_scanned": domain_dirs.len(),
            "sitemaps_scanned": total_sitemaps,
            "index_files": total_indexes,
            "loc_entries_scanned": total_locs,
            "xhtml_links_scanned": total_xhtml_links,
            "xhtml_links_per_loc": xhtml_links_per_loc(total_xhtml_links, total_locs),
            "size_bytes": total_bytes,
            "size_human": human_size(total_bytes),
            "files_skipped": total_skipped,
        }
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn xhtml_links_per_loc(xhtml_links: usize, locs: usize) -> f64 {
    if locs == 0 {
        0.0
    } else {
        (xhtml_links as f64 / locs as f64 * 100.0).round() / 100.0
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.2} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_formatting() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.50 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.00 MiB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.00 GiB");
    }
}
