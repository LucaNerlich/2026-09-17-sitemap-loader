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

### Find

Searches previously downloaded sitemap files under `./sitemaps/` for `<loc>` entries containing a substring, and prints JSON matches + stats.

```
sitemapper find <substring> [--domain <domain>]
```

`--domain` restricts the search to `./sitemaps/<domain>`; omit it to search every downloaded domain.
