# WordPress on goro-httpd + SQLite

This sample runs a stock WordPress on goro, served by `goro-httpd` (goro's
HTTP/HTTPS server SAPI, built on [httpsd](https://crates.io/crates/httpsd)),
with the database in SQLite through the official
[SQLite Database Integration](https://wordpress.org/plugins/sqlite-database-integration/)
plugin (which uses PDO SQLite — goro's pure-Rust SQLite port). No MySQL, no
web server, no C libraries.

## Quick start

```sh
samples/wordpress/setup.sh   # download WordPress + the SQLite plugin, write wp-config.php
samples/wordpress/run.sh     # build goro + goro-httpd if needed, then serve
```

Then open <http://127.0.0.1:8080/> (or <https://127.0.0.1:8443/>, with a
self-signed certificate) and go through the WordPress installer.

To check that everything works without a browser, run the smoke test while
the server is running:

```sh
samples/wordpress/smoke.sh   # http://127.0.0.1:8080 https://127.0.0.1:8443
```

It installs the site (if needed), then checks the front page, static files,
login and the dashboard, sets pretty permalinks (`/%postname%/`), publishes
a post through the REST API and reads it back at its permalink, posts a
comment, uploads a text file and an image (multipart; the thumbnails are
made with goro's GD) and logs in again over HTTPS (secure cookies, HTTP/2). It also fails if `wp-content/debug.log` gets any PHP
diagnostics. It exits non-zero on the first failure.

## What the scripts do

- `setup.sh` downloads the latest WordPress and the SQLite plugin into
  `site/` (gitignored; the downloads are cached in `.cache/`). Proxies from
  `https_proxy`/`http_proxy` are used for the downloads and are configured in
  `wp-config.php` (`WP_PROXY_HOST`) for WordPress' own requests (`OFFLINE=1`
  blocks those instead). It installs the plugin's `db.copy` as the
  `wp-content/db.php` drop-in, and writes a `wp-config.php` with:
  - the database in `wp-content/database/.ht.sqlite` (goro-httpd never
    serves dotfiles),
  - `WP_HOME`/`WP_SITEURL` derived from the request, so the site answers on
    both the HTTP and the HTTPS port,
  - `WP_DEBUG` + `WP_DEBUG_LOG` (`wp-content/debug.log`) on, display off
    (`WP_DEBUG=0` turns them off).

  It also adds a one-line must-use plugin enabling pretty permalinks:
  WordPress only offers them for servers it recognizes (Apache with
  mod_rewrite, nginx, Caddy, IIS), and goro-httpd routes requests for
  missing files to `/index.php` like WordPress' `.htaccess` does.
- `run.sh` makes a self-signed certificate in `certs/` with goro's
  ext/openssl (`mkcert.php`) and starts
  `goro-httpd -t site -l 127.0.0.1:8080 -s 127.0.0.1:8443 --tls-cert ... --tls-key ...`.
  `HTTP_ADDR` / `HTTPS_ADDR` change the listeners; extra arguments are
  passed to goro-httpd (`goro-httpd --help` lists them).
- `smoke.sh [HTTP_BASE] [HTTPS_BASE]` is the end-to-end test. It also runs
  against other servers, e.g. php's built-in one
  (`CHECK_DOTFILES=0 smoke.sh http://127.0.0.1:8000 ""`), to compare.

## goro-httpd

```
goro-httpd -t DOCROOT [-l ADDR]... [-s ADDR --tls-cert FILE --tls-key FILE | --self-signed]
           [-w WORKERS] [-c php.ini] [-d key=value]...
```

It serves the document root like php-fpm behind nginx/Apache: `index.php`
(then `index.html`) directory indexes, `/script.php/path/info`, static files
with their MIME types (ETag/Last-Modified, ranges), gzip/deflate
compression, HTTP/1.1 keep-alive and HTTP/2 over TLS, and a front
controller for missing files (`--front-controller /index.php`, the
default). `$_SERVER` has the usual CGI variables (`SCRIPT_NAME`,
`SCRIPT_FILENAME`, `PATH_INFO`, `PHP_SELF`, `REQUEST_URI`, `QUERY_STRING`,
`HTTP_*`, `HTTPS=on`, `REMOTE_ADDR`, `SERVER_NAME`/`SERVER_PORT`, ...);
`PHP_SAPI` is `goro-httpd`.

Each request runs in a fresh engine on one of the worker threads; the
opcache script cache is per worker, so a worker's first requests compile
WordPress (about 0.45 s for the front page) and later ones reuse it (about
0.27 s; php 8.5 takes 0.12-0.17 s without opcache, 0.03 s with it). With
`-d opcache.file_cache=/some/dir` the compiled files are also written to
that directory, and the first requests of the other workers and of later
servers load them instead of compiling WordPress again (about a quarter
less work for such a request: about 0.35 s). As the
timers of `max_execution_time` are per process, the limit is not enforced.
