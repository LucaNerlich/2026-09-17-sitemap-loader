use anyhow::Result;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;

pub enum SitemapKind {
    Index(Vec<String>),
    UrlSet(Vec<String>),
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

pub fn parse(bytes: &[u8]) -> Result<SitemapKind> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);

    let mut locs = Vec::new();
    let mut is_index = false;
    let mut in_loc = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) => match e.local_name().as_ref() {
                "sitemapindex" => is_index = true,
                "loc" => in_loc = true,
                _ => {}
            },
            Event::End(e) => {
                if e.local_name().as_ref() == "loc" {
                    in_loc = false;
                }
            }
            Event::Text(t) if in_loc => {
                locs.push(unescape(&t)?.into_owned());
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(if is_index {
        SitemapKind::Index(locs)
    } else {
        SitemapKind::UrlSet(locs)
    })
}
