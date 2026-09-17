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

If `<url>` has no path (just a domain, e.g. `https://www.sap.com`), `fetch` looks up `Sitemap:` entries in that domain's `/robots.txt` instead and fetches every tree declared there:

```
sitemapper fetch https://www.sap.com
```

Some sites reject requests with no/generic `User-Agent` header (a 403 from the WAF, not from robots.txt rules). If a fetch gets rejected, override the header:

```
sitemapper fetch <url> --user-agent "curl/8.7.1"
```

If the sitemap is behind HTTP Basic Auth, pass credentials with `-u`/`--user` (applied to every request in the fetch tree, same as curl):

```
sitemapper fetch <url> -u username:password
```

Fetching a large sitemap tree can trip a site's bot-detection (Akamai, etc.) — you'll see a run of `403 Forbidden` failures partway through, often with `server: AkamaiGHost` and an `Access Denied` body if you check with `curl -v`. That's a behavioral block from too many requests too fast, not a User-Agent problem, and retrying immediately with a different UA won't help — it usually needs a cool-down before the block clears. Crawl more politely with:

```
sitemapper fetch <url> --concurrency 2 --delay-ms 500
```

`--concurrency` caps simultaneous requests (default 8), `--delay-ms` adds a wait before each one (default 0).

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

### Stats

Summarizes what's already downloaded under `./sitemaps/`, per domain: sitemap/index file counts, total loc entries, `<xhtml:link>` (hreflang alternate) counts with a per-loc ratio, and total on-disk size.

```
sitemapper stats [--domain <domain>]
```
