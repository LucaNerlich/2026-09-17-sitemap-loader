use crate::sitemap::{self, SitemapKind};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Default)]
struct Stats {
    sitemaps_extracted: usize,
    files_skipped: usize,
    loc_entries_found: usize,
}

/// Copies real sitemap files out of an extracted AEM content package into the
/// same flat `./sitemaps/<domain>/*.xml` layout `fetch` produces, so `find`
/// works the same way regardless of where the sitemaps came from.
pub fn run(source: &Path) -> Result<()> {
    let start = Instant::now();
    if !source.is_dir() {
        bail!("source directory not found: {}", source.display());
    }

    let dest_root = PathBuf::from("sitemaps");
    let mut stats = Stats::default();
    let mut used_paths = HashSet::new();

    for path in sitemap::find_xml_files(source) {
        extract_one(source, &path, &dest_root, &mut used_paths, &mut stats)?;
    }

    println!("Extracted sitemaps from {}", source.display());
    println!("  duration:            {:.2}s", start.elapsed().as_secs_f64());
    println!("  sitemaps extracted:  {}", stats.sitemaps_extracted);
    println!("  files skipped:       {}", stats.files_skipped);
    println!("  loc entries found:   {}", stats.loc_entries_found);
    println!("  saved to:            {}", dest_root.display());

    Ok(())
}

/// AEM content packages store per-file JCR metadata as `.content.xml` (both
/// loose and inside `<name>.xml.dir/` folders) alongside the real sitemap
/// files; only files whose root element is `<urlset>`/`<sitemapindex>` are
/// genuine sitemaps, so `sitemap::parse` itself is what filters those out.
fn extract_one(
    root: &Path,
    path: &Path,
    dest_root: &Path,
    used_paths: &mut HashSet<PathBuf>,
    stats: &mut Stats,
) -> Result<()> {
    let Ok(bytes) = std::fs::read(path) else {
        stats.files_skipped += 1;
        return Ok(());
    };
    let Ok(parsed) = sitemap::parse(&bytes) else {
        stats.files_skipped += 1;
        return Ok(());
    };

    let rel = path.strip_prefix(root).context("path escaped source root")?;
    let mut components = rel.components();
    let domain = components.next().context("path has no domain component")?.as_os_str().to_string_lossy();
    let rest: Vec<_> = components.map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let base_name = sitemap::sanitize_filename(&rest.join("-"));

    let dest_dir = dest_root.join(domain.as_ref());
    std::fs::create_dir_all(&dest_dir)?;

    let mut dest = dest_dir.join(&base_name);
    let mut i = 1;
    while !used_paths.insert(dest.clone()) {
        dest = dest_dir.join(format!("{i}-{base_name}"));
        i += 1;
    }
    std::fs::write(&dest, &bytes)?;

    stats.sitemaps_extracted += 1;
    if let SitemapKind::UrlSet(locs) = parsed.kind {
        stats.loc_entries_found += locs.len();
    }
    Ok(())
}
