use anyhow::{bail, Result};
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;
use std::path::{Path, PathBuf};

pub enum SitemapKind {
    Index(Vec<String>),
    UrlSet(Vec<String>),
}

pub struct ParsedSitemap {
    pub kind: SitemapKind,
    /// Count of `<xhtml:link>` elements (hreflang alternates), which sitemaps
    /// commonly have several of per `<url>` entry.
    pub xhtml_links: usize,
}

/// Recursively collects every `*.xml` file under `dir`. Skips over unreadable
/// entries rather than failing, since callers only care about the files that
/// are actually there.
pub fn find_xml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_xml_files(dir, &mut out);
    out
}

fn collect_xml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            collect_xml_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("xml") {
            out.push(path);
        }
    }
}

/// Resolves which `./sitemaps/<domain>` directories a `--domain`-taking
/// command should scan: just that one (erroring if it doesn't exist), or
/// every domain directory under `sitemaps/` if none was given.
pub fn resolve_domain_dirs(sitemaps_dir: &Path, domain: Option<&str>) -> Result<Vec<PathBuf>> {
    match domain {
        Some(d) => {
            let dir = sitemaps_dir.join(d);
            if !dir.is_dir() {
                bail!("no sitemaps found for domain '{d}' (expected directory {})", dir.display());
            }
            Ok(vec![dir])
        }
        None => Ok(std::fs::read_dir(sitemaps_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default()),
    }
}

/// Makes `name` safe to use as a saved file name, ensuring it ends in `.xml`.
pub fn sanitize_filename(name: &str) -> String {
    let name = name.strip_suffix(".gz").unwrap_or(name);
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    if cleaned.to_ascii_lowercase().ends_with(".xml") {
        cleaned
    } else {
        format!("{cleaned}.xml")
    }
}

/// Sitemaps are routinely served gzip-compressed regardless of URL extension;
/// detect via magic bytes rather than trusting `.gz`/Content-Type.
pub fn maybe_gunzip(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        if flate2::read::GzDecoder::new(&bytes[..])
            .read_to_end(&mut out)
            .is_ok()
        {
            return out;
        }
    }
    bytes
}

pub fn parse(bytes: &[u8]) -> Result<ParsedSitemap> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);

    let mut locs = Vec::new();
    let mut root_is_index = None;
    let mut in_loc = false;
    let mut xhtml_links = 0usize;
    // quick-xml splits text containing an entity (e.g. `a=1&amp;b=2`, common
    // in URLs with query strings) into separate Text/GeneralRef events, so a
    // single <loc> value must be accumulated across events, not pushed per-event.
    let mut current_loc = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => {
                let name = e.local_name();
                let name = name.as_ref();
                if root_is_index.is_none() {
                    root_is_index = Some(classify_root(name)?);
                } else if name == "loc" {
                    in_loc = true;
                    current_loc.clear();
                } else if is_xhtml_link(e.name().as_ref()) {
                    xhtml_links += 1;
                }
            }
            Event::Empty(e) => {
                // A childless root (e.g. `<sitemapindex/>`) is `Empty`, not
                // `Start`+`End`; a self-closing `<loc/>` has no text, so it
                // needs no `in_loc` tracking either way.
                if root_is_index.is_none() {
                    root_is_index = Some(classify_root(e.local_name().as_ref())?);
                } else if is_xhtml_link(e.name().as_ref()) {
                    xhtml_links += 1;
                }
            }
            Event::End(e) => {
                if e.local_name().as_ref() == "loc" && in_loc {
                    locs.push(std::mem::take(&mut current_loc));
                    in_loc = false;
                }
            }
            Event::Text(t) if in_loc => {
                current_loc.push_str(&unescape(&t)?);
            }
            Event::GeneralRef(r) if in_loc => {
                if let Some(ch) = r.resolve_char_ref()? {
                    current_loc.push(ch);
                } else if let Some(resolved) = quick_xml::escape::resolve_xml_entity(&r) {
                    current_loc.push_str(resolved);
                }
                // An unresolvable named entity (custom DTD) has no sane
                // substitution; drop it rather than corrupt the URL.
            }
            _ => {}
        }
        buf.clear();
    }

    let kind = match root_is_index {
        Some(true) => SitemapKind::Index(locs),
        Some(false) => SitemapKind::UrlSet(locs),
        None => bail!("empty or non-XML content"),
    };
    Ok(ParsedSitemap { kind, xhtml_links })
}

fn is_xhtml_link(qualified_name: &str) -> bool {
    qualified_name == "xhtml:link"
}

fn classify_root(name: &str) -> Result<bool> {
    match name {
        "sitemapindex" => Ok(true),
        "urlset" => Ok(false),
        other => bail!("not a sitemap (root element is <{other}>)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locs(parsed: ParsedSitemap) -> Vec<String> {
        match parsed.kind {
            SitemapKind::Index(locs) | SitemapKind::UrlSet(locs) => locs,
        }
    }

    #[test]
    fn parses_urlset_with_multiple_entries() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url><loc>https://example.com/products/widget.html</loc></url>
              <url><loc>https://example.com/products/gadget.html</loc></url>
            </urlset>"#;

        let parsed = parse(xml).unwrap();
        assert!(matches!(parsed.kind, SitemapKind::UrlSet(_)));
        assert_eq!(
            locs(parsed),
            vec![
                "https://example.com/products/widget.html",
                "https://example.com/products/gadget.html",
            ]
        );
    }

    #[test]
    fn parses_sitemapindex_child_locations() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <sitemap><loc>https://example.com/sitemap-1.xml</loc></sitemap>
              <sitemap><loc>https://example.com/sitemap-2.xml</loc></sitemap>
            </sitemapindex>"#;

        let parsed = parse(xml).unwrap();
        assert!(matches!(parsed.kind, SitemapKind::Index(_)));
        assert_eq!(
            locs(parsed),
            vec!["https://example.com/sitemap-1.xml", "https://example.com/sitemap-2.xml"]
        );
    }

    #[test]
    fn unescapes_entities_in_loc_urls() {
        let xml = br#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url><loc>https://example.com/search?a=1&amp;b=2</loc></url>
            </urlset>"#;

        assert_eq!(locs(parse(xml).unwrap()), vec!["https://example.com/search?a=1&b=2"]);
    }

    /// Regression test: a childless root like `<sitemapindex/>` is emitted by
    /// quick-xml as `Event::Empty`, not `Event::Start` + `Event::End`. This
    /// previously fell through every match arm and was misclassified as "not
    /// a sitemap".
    #[test]
    fn parses_self_closing_empty_sitemapindex() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"/>"#;

        let parsed = parse(xml).unwrap();
        assert!(matches!(parsed.kind, SitemapKind::Index(_)));
        assert!(locs(parsed).is_empty());
    }

    #[test]
    fn empty_urlset_has_no_locs() {
        let xml = br#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"></urlset>"#;
        assert!(locs(parse(xml).unwrap()).is_empty());
    }

    /// Real-world sitemaps use `<xhtml:link>` for hreflang alternates, often
    /// several self-closing ones per `<url>` entry (see stats' xhtml_links count).
    #[test]
    fn counts_self_closing_xhtml_links_per_url() {
        let xml = br#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
              <url>
                <loc>https://example.com/products/widget.html</loc>
                <xhtml:link rel="alternate" hreflang="en-US" href="https://example.com/us/products/widget.html"/>
                <xhtml:link rel="alternate" hreflang="de-DE" href="https://example.com/de/products/widget.html"/>
              </url>
              <url>
                <loc>https://example.com/products/gadget.html</loc>
                <xhtml:link rel="alternate" hreflang="en-US" href="https://example.com/us/products/gadget.html"/>
              </url>
            </urlset>"#;

        let parsed = parse(xml).unwrap();
        assert_eq!(parsed.xhtml_links, 3);
        assert_eq!(
            locs(parsed),
            vec!["https://example.com/products/widget.html", "https://example.com/products/gadget.html"]
        );
    }

    #[test]
    fn xhtml_link_count_does_not_affect_loc_extraction() {
        let xml = br#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
              <url>
                <loc>https://example.com/products/widget.html</loc>
                <xhtml:link rel="alternate" hreflang="de-DE" href="https://example.com/de/products/widget.html"/>
              </url>
            </urlset>"#;

        let parsed = parse(xml).unwrap();
        assert_eq!(parsed.xhtml_links, 1);
        match parsed.kind {
            SitemapKind::UrlSet(locs) => assert_eq!(locs, vec!["https://example.com/products/widget.html"]),
            SitemapKind::Index(_) => panic!("expected UrlSet"),
        }
    }

    #[test]
    fn no_xhtml_links_counts_zero() {
        let xml = br#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url><loc>https://example.com/products/widget.html</loc></url>
            </urlset>"#;

        assert_eq!(parse(xml).unwrap().xhtml_links, 0);
    }

    /// AEM content packages store per-file JCR metadata as XML rooted at
    /// `<jcr:root>` alongside real sitemaps (see `extract`); those must be
    /// rejected, not silently treated as an empty sitemap.
    #[test]
    fn rejects_non_sitemap_xml_root() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <jcr:root xmlns:jcr="http://www.jcp.org/jcr/1.0" jcr:primaryType="nt:file"/>"#;

        assert!(parse(xml).is_err());
    }

    #[test]
    fn rejects_non_xml_content() {
        assert!(parse(b"{\"not\": \"xml\"}").is_err());
        assert!(parse(b"").is_err());
    }

    #[test]
    fn gunzip_roundtrips_gzip_content_and_passes_through_plain_content() {
        use std::io::Write;

        let plain = b"<urlset></urlset>".to_vec();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&plain).unwrap();
        let gzipped = encoder.finish().unwrap();

        assert_eq!(maybe_gunzip(gzipped), plain);
        assert_eq!(maybe_gunzip(plain.clone()), plain);
    }

    #[test]
    fn sanitize_filename_cases() {
        assert_eq!(sanitize_filename("sitemap.xml"), "sitemap.xml");
        assert_eq!(sanitize_filename("sitemap.xml.gz"), "sitemap.xml");
        assert_eq!(sanitize_filename("sitemap"), "sitemap.xml");
        assert_eq!(sanitize_filename("weird name?.xml"), "weird_name_.xml");
    }
}
