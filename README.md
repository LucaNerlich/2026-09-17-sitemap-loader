# sitemapper

CLI to download XML sitemaps (or sitemap indexes) and search them for URLs matching a substring.

## Install

```
cargo install --path .
```

This builds a release binary and copies it to `~/.cargo/bin/sitemapper`, which `rustup` already put on your `PATH`.

To pick up code changes later, re-run the same command (add `--force` if the version didn't change). To remove it: `cargo uninstall sitemapper`.

## Usage

### Fetch

Downloads a sitemap or sitemap index (recursing into all child sitemaps, up to 8 concurrent requests) into `./sitemaps/<domain>/`.

```
sitemapper fetch <url>
```

Some sites reject requests with no/generic `User-Agent` header (a 403 from the WAF, not from robots.txt rules). If a fetch gets rejected, override the header:

```
sitemapper fetch <url> --user-agent "curl/8.7.1"
```

If the sitemap is behind HTTP Basic Auth, pass credentials with `-u`/`--user` (applied to every request in the fetch tree, same as curl):

```
sitemapper fetch <url> -u username:password
```

### Extract

If a site's sitemap can't be fetched over HTTP (e.g. it errors out server-side) but you have an AEM content package export of it on disk, `extract` pulls the real sitemap files out of the package and copies them into the same `./sitemaps/<domain>/` layout `fetch` produces — so `find` works the same way regardless of source.

```
sitemapper extract [source]
```

`source` defaults to `./content-package`. AEM packages also contain JCR metadata XML files (`.content.xml`, and `<name>.xml.dir/` folders) alongside the real sitemaps; those are detected and skipped automatically (only files whose root element is `<urlset>`/`<sitemapindex>` are extracted). Locale/section subfolders in the package (e.g. `en_us/sitemap-1.xml`) are flattened into a single prefixed filename (`en_us-sitemap-1.xml`) rather than preserved as nested directories.

### Find

Searches previously downloaded sitemap files under `./sitemaps/` for `<loc>` entries containing a substring, and prints JSON matches + stats.

```
sitemapper find <substring> [--domain <domain>]
```

`--domain` restricts the search to `./sitemaps/<domain>`; omit it to search every downloaded domain.
