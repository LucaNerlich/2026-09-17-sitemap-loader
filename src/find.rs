use crate::sitemap::{self, SitemapKind};
use anyhow::{bail, Result};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Default, Debug, PartialEq)]
struct SearchResult {
    matches: Vec<String>,
    sitemaps_scanned: usize,
    loc_entries_scanned: usize,
}

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

    let result = search(&domain_dirs, substring);

    let matches_found = result.matches.len();
    let output = json!({
        "matches": result.matches,
        "stats": {
            "duration_ms": start.elapsed().as_millis(),
            "sitemaps_scanned": result.sitemaps_scanned,
            "loc_entries_scanned": result.loc_entries_scanned,
            "matches_found": matches_found,
        }
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Only `<urlset>` files count as page locations; `<sitemapindex>` files
/// (and anything else `sitemap::parse` rejects, e.g. AEM JCR metadata) are
/// skipped since their `<loc>` values point at other sitemaps, not pages.
fn search(dirs: &[PathBuf], substring: &str) -> SearchResult {
    let mut result = SearchResult::default();

    for dir in dirs {
        for path in sitemap::find_xml_files(dir) {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(SitemapKind::UrlSet(locs)) = sitemap::parse(&bytes) else { continue };

            result.sitemaps_scanned += 1;
            result.loc_entries_scanned += locs.len();
            result.matches.extend(locs.into_iter().filter(|loc| loc.contains(substring)));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A scratch directory under the OS temp dir, removed on drop, so tests
    /// don't depend on or pollute the crate's own `./sitemaps`.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("sitemapper-find-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel_path: &str, content: &str) {
            let path = self.0.join(rel_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const URLSET_A: &str = r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
        <url><loc>https://example.com/resources/faq.html</loc></url>
        <url><loc>https://example.com/blogs/launch.html</loc></url>
    </urlset>"#;

    const URLSET_B: &str = r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
        <url><loc>https://example.com/resources/pricing.html</loc></url>
    </urlset>"#;

    const SITEMAP_INDEX: &str = r#"<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
        <sitemap><loc>https://example.com/resources/sitemap.xml</loc></sitemap>
    </sitemapindex>"#;

    const JCR_METADATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
        <jcr:root xmlns:jcr="http://www.jcp.org/jcr/1.0" jcr:primaryType="nt:file"/>"#;

    #[test]
    fn matches_substring_across_multiple_sitemaps_in_a_domain() {
        let dir = TempDir::new();
        dir.write("en_us/sitemap-1.xml", URLSET_A);
        dir.write("en_us/sitemap-2.xml", URLSET_B);

        let mut result = search(std::slice::from_ref(&dir.0), "/resources/");
        result.matches.sort();

        assert_eq!(result.sitemaps_scanned, 2);
        assert_eq!(result.loc_entries_scanned, 3);
        assert_eq!(
            result.matches,
            vec!["https://example.com/resources/faq.html", "https://example.com/resources/pricing.html"]
        );
    }

    #[test]
    fn excludes_sitemap_index_locs_and_jcr_metadata() {
        let dir = TempDir::new();
        dir.write("sitemap-1.xml", URLSET_A);
        dir.write("sitemap-index.xml", SITEMAP_INDEX);
        dir.write("sitemap-1.xml.dir/.content.xml", JCR_METADATA);

        let result = search(std::slice::from_ref(&dir.0), "/resources/");

        // Only the urlset file counts; the index's own <loc> (pointing at
        // another sitemap file) and the JCR metadata are both excluded.
        assert_eq!(result.sitemaps_scanned, 1);
        assert_eq!(result.matches, vec!["https://example.com/resources/faq.html"]);
    }

    #[test]
    fn no_match_returns_empty_result_not_an_error() {
        let dir = TempDir::new();
        dir.write("sitemap-1.xml", URLSET_A);

        let result = search(std::slice::from_ref(&dir.0), "/nonexistent-path/");

        assert!(result.matches.is_empty());
        assert_eq!(result.sitemaps_scanned, 1);
    }

    #[test]
    fn searches_across_multiple_domain_dirs() {
        let prod = TempDir::new();
        prod.write("sitemap-1.xml", URLSET_A);
        let qa = TempDir::new();
        qa.write("sitemap-1.xml", URLSET_B);

        let result = search(&[prod.0.clone(), qa.0.clone()], "/resources/");

        assert_eq!(result.sitemaps_scanned, 2);
        assert_eq!(result.matches.len(), 2);
    }
}
