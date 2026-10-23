# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development, aiming at full PHP 8.5 parity. The language core (OOP, traits, enums,
closures, generators, fibers, property hooks, lazy objects, typed properties and references,
match, readonly, asymmetric visibility, pipe operator) and a large part of the standard
extensions run the official php-src test suite.

**php-src 8.5.4 test suite** (full run, master `81f84f02`, 2026-10-23):

| Metric | Count |
|---|---|
| Total tests | 21696 (the generic PDO tests run once per driver) |
| Pass | 20171 (93.0% of all, **99.4% of runnable**) |
| Fail / Error | 127 |
| Skip / XFail | 1398 (870 skip on real PHP on Linux too: Windows-only, 32-bit, online, …; 320 out-of-scope extensions: ffi, odbc, snmp, enchant, pdo_firebird/dblib/odbc; 213 php-fpm and phpdbg) |

CI runs the whole suite in 8 parallel shards on every push (`GORO_PHPT_SHARD=i/n`), plus
`cargo fmt --check` and `cargo clippy -D warnings`.

**Pass rates by directory** (pass / run, skips excluded):

| Area | Pass | Run | Rate |
|---|---|---|---|
| Zend/tests | 5254 | 5255 | 99.98% |
| tests/ (lang, classes, basic, …) | 826 | 832 | 99.3% |
| ext/standard | 3373 | 3376 | 99.9% |
| ext/spl | 779 | 779 | 100% |
| ext/date | 677 | 677 | 100% |
| ext/reflection | 492 | 493 | 99.8% |
| ext/dom (incl. HTML5 `Dom\HTMLDocument`) | 849 | 850 | 99.9% |
| ext/simplexml / xml / xmlreader / xmlwriter / libxml | 339 | 344 | 98.5% |
| ext/soap | 574 | 574 | 100% |
| ext/gd (bundled libgd port; PNG/GIF/JPEG/WebP/AVIF/BMP/TGA/XBM/XPM/WBMP/GD2, FreeType) | 308 | 308 | 100% |
| ext/xsl (libxslt + libexslt port) | 76 | 76 | 100% |
| ext/mbstring | 415 | 415 | 100% |
| ext/intl (on the `intl` crate) | 426 | 428 | 99.5% |
| ext/iconv (glibc iconv emulation) | 68 | 68 | 100% |
| ext/openssl (incl. TLS streams, on purecrypto) | 208 | 213 | 97.7% |
| ext/sodium (on purecrypto) | 30 | 30 | 100% |
| ext/mysqli (embedded MySQL fixture) | 420 | 420 | 100% |
| ext/pdo (generic tests, run against MySQL, PostgreSQL and SQLite) | 369 | 372 | 99.2% |
| ext/sqlite3 / pdo_sqlite (goro-sqlite, a SQLite 3.54.1 port) | 174 | 175 | 99.4% |
| ext/pdo_mysql / pdo_pgsql | 219 | 219 | 100% |
| ext/pgsql (libpq port, embedded PostgreSQL fixture) | 96 | 96 | 100% |
| ext/ldap (pure-Rust libldap port, in-process LDAP server fixture) | 131 | 131 | 100% |
| ext/session | 257 | 258 | 99.6% |
| ext/pcre (PCRE2 port, JIT stack-limit emulation) | 161 | 161 | 100% |
| ext/hash / random / zlib / bz2 | 305 | 306 | 99.7% |
| ext/phar (phar, tar, zip formats) | 542 | 542 | 100% |
| ext/zip (libzip port) / exif / fileinfo (libmagic port) | 265 | 265 | 100% |
| ext/json | 88 | 88 | 100% |
| ext/bcmath / filter / gmp (bcmath and gmp on puremp) | 379 | 379 | 100% |
| ext/ctype / tokenizer | 102 | 102 | 100% |
| ext/uri (uriparser port + lexbor WHATWG URL parser port) | 309 | 309 | 100% |
| ext/posix / pcntl / calendar / ftp / gettext / shmop / sysv* | 253 | 253 | 100% |
| ext/curl (libcurl emulation) / sockets | 246 | 246 | 100% |
| ext/opcache (script cache, preloading; 59 of the fails expect optimizer opcode dumps) | 819 | 881 | 93.0% |
| ext/zend_test (engine test hooks) | 145 | 148 | 98.0% |
| ext/dba (cdb, cdb_make, inifile, flatfile) / readline | 53 | 53 | 100% |
| ext/tidy (tidy-html5 5.8.0 port) | 50 | 50 | 100% |
| sapi/cli (incl. `php -S` and `php -a`) | 102 | 103 | 99.0% |
| sapi/cgi (goro runs as php-cgi under a `*cgi*` name) | 22 | 23 | 95.7% |
| sapi/fpm (php-fpm; 7 skips need root, AppArmor or FreeBSD) | 133 | 133 | 100% |
| PECL uuid 1.3 (`tests/pecl/uuid`) | 14 | 14 | 100% |
| PECL mailparse 3.2 (`tests/pecl/mailparse`, 1 upstream XFAIL) | 38 | 39 | 97.4% |
| PECL imagick 3.8 (`tests/pecl/imagick`, ImageMagick 7.1 semantics; 11 skips) | 322 | 338 | 95.3% |


*The test runner supports recursive directories, single files, SKIPIF/INI/ENV/ARGS sections,
EXPECTF/EXPECTREGEX matching, XFAIL, per-test timeouts, embedded MySQL and PostgreSQL fixtures for ext/mysqli, ext/pgsql, ext/pdo_mysql, ext/pdo_pgsql and the generic ext/pdo tests, and an in-process LDAP server fixture for ext/ldap.*

## Features

Highlights beyond the core language:

- **Bytecode VM** with a PHP-style symbol table, real references, by-ref returns,
  copy-on-write-like array separation, php-src destructor timing and shutdown order, and a
  Zend-style cycle collector (`gc_collect_cycles`, auto-collection, `gc_status`).
- **Closures** are real `Closure` objects; first-class callables (`strlen(...)`).
- **Fibers**, generators (`yield from`, finally on destruction), property hooks (8.4),
  lazy objects (8.4), asymmetric visibility (8.4), pipe operator `|>` (8.5), `Uri\*` (8.5).
- **Builtin arginfo** generated from php-src `*.stub.php`: named arguments, argument-count
  and zpp-style type errors, internal frames in backtraces, Reflection of internal
  functions and classes.
- **Streams**: userspace stream wrappers, filters (incl. user filters), sockets
  (tcp/udp/unix), `stream_select`, `proc_open`, TLS (`ssl://`, `tls://`,
  `stream_socket_enable_crypto`) and `https://`.
- **Extensions** (all pure Rust, no C FFI): standard, date (timelib port), pcre (PCRE2 port),
  hash, random, zlib (zlib port, byte-identical output), json, ctype, mbstring, spl,
  reflection, session, dom/simplexml/xml/xmlreader/xmlwriter (libxml2-style tree, lexbor
  HTML5 parser port), openssl (on [purecrypto](https://crates.io/crates/purecrypto)),
  mysqli (mysqlnd port), pgsql (libpq port: SCRAM/MD5 auth, TLS, COPY, async, large objects),
  PDO with pdo_mysql (over the same mysqlnd) and pdo_pgsql (over the same libpq), gmp (on [puremp](https://crates.io/crates/puremp), with GMP's exact Mersenne Twister streams), sockets, curl, bz2 (on
  [compcol](https://crates.io/crates/compcol)'s bzip2, libbzip2's streaming/flush semantics), tokenizer,
  bcmath (libbcmath port, big-integer kernels on puremp), filter, iconv (glibc iconv emulation), calendar, posix,
  pcntl (fork, signals with async dispatch), ftp (with FTPS), shmop/sysvsem/sysvshm/sysvmsg,
  gettext (libintl emulation reading .mo catalogs), sodium (libsodium API on purecrypto,
  argon2i/argon2id `password_hash()`), soap (WSDL/XML Schema, SoapClient over HTTP/HTTPS with
  keep-alive, proxies, Basic/Digest auth, compression; SoapServer), phar (phar/tar/zip archives, phar:// wrapper, signatures), zip (libzip port, zip://; store, deflate, bzip2, LZMA, XZ and Zstandard methods, the latter on compcol),
  exif, fileinfo (libmagic 5.46 port with the bundled magic database), uri (RFC 3986 + WHATWG URL),
  intl (on the [intl](https://crates.io/crates/intl) crate), xsl (an XSLT 1.0 processor ported
  from libxslt 1.1.45 with all of EXSLT; it matches libxslt on its own test suite and the DocBook
  stylesheets), gd (port of the bundled libgd with byte-identical PNG/GIF/JPEG output; FreeType
  text on skrifa, WebP/AVIF on the OxideAV codecs), dba (cdb/cdb_make, inifile and flatfile
  handlers), readline (GNU readline emulation, with the `php -a` interactive shell),
  uri (`Uri\Rfc3986\Uri` on a uriparser port, `Uri\WhatWg\Url` on a port of lexbor's WHATWG
  URL parser), tidy (a port of tidy-html5 5.8.0 matching libtidy's output and messages),
  Zend OPcache (an in-memory script cache with the opcache_*() API, preloading and the file
  cache's entries; no optimizer or JIT, the directives are reported as php-src does),
  sqlite3 and pdo_sqlite (on goro-sqlite, a pure-Rust port of SQLite 3.54.1 incl. WAL, JSON and virtual tables),
  zend_test; and the PECL extensions uuid (RFC 9562 UUIDs v1/v3/v4/v5/v6/v7, the API of
  util-linux libuuid), mailparse (RFC 822 address parsing, an incremental MIME parser with
  part extraction and transfer decoding, uuencoded attachments) and imagick (Imagick,
  ImagickDraw, ImagickPixel, ImagickPixelIterator, ImagickKernel with ImageMagick 7 Q16
  semantics: PNG/JPEG/GIF/WebP/BMP/TIFF/ICO/TGA/QOI/PNM and raw formats on the OxideAV codecs,
  SVG on oxideav-svg, pseudo images, geometry, ImageMagick's resize filters, every colourspace,
  compositing, distortions, filters, ordered and error-diffusion dithering, Fourier transforms,
  8BIM clipping paths, drawing (patterns, text decorations) and text on oxideav-raster and
  skrifa; no ICC colour management yet).

### Not Yet Implemented
- ffi, odbc, snmp, enchant (out of scope for now)
- PCRE JIT (the PCRE2 port is the interpreter only; `PCRE_JIT_SUPPORT` is false)
- The opcache optimizer and JIT (`opcache.opt_debug_level` opcode dumps)

## Building

```bash
cargo build --release
```

## Usage

```bash
# Run a PHP file
./target/release/goro script.php

# Run inline PHP code
./target/release/goro -r 'echo "Hello, World!\n";'

# Run PHPT tests
./target/release/goro --test /path/to/tests/

# goro's own regression tests
./target/release/goro --test tests/goro/
```

### SAPIs

- **CLI** (`goro`, with `-S` and `-a`)
- **php-cgi**: goro run under a name containing `cgi` (e.g. a `php-cgi`
  symlink) is the CGI SAPI, with its FastCGI mode (`-b 127.0.0.1:9000`,
  `PHP_FCGI_CHILDREN`, `PHP_FCGI_MAX_REQUESTS`)
- **php-fpm**: goro run under a name containing `fpm` is the FastCGI Process
  Manager: php-fpm.conf pools (`static`, `dynamic`, `ondemand`), TCP/unix
  sockets, signals (USR1 reopens logs, USR2 reloads), the error, access and
  slow logs, `catch_workers_output`, the status and ping pages,
  `fpm_get_status()` and `fastcgi_finish_request()`:
  ```bash
  ln -s goro target/release/php-fpm
  ./target/release/php-fpm -y php-fpm.conf -F
  ```
- **goro-httpd**, below

### Web server SAPI (goro-httpd)

`goro-httpd` serves a document root over HTTP and HTTPS the way php-fpm
behind nginx (or mod_php) would, on the pure-Rust [httpsd](https://crates.io/crates/httpsd)
server core (HTTP/1.1, HTTP/2 over TLS via purecrypto, gzip):

```bash
./target/release/goro-httpd -t /var/www -l 127.0.0.1:8080 \
    -s 127.0.0.1:8443 --tls-cert cert.pem --tls-key key.pem   # or --self-signed
```

`index.php` directory indexes, `/script.php/path/info`, static files with
their MIME types, and requests for missing files routed to a front
controller (`/index.php`, like WordPress' `.htaccess` rewrite); `$_SERVER`
has the CGI variables (`SCRIPT_NAME`, `PATH_INFO`, `REQUEST_URI`, `HTTPS`,
`REMOTE_ADDR`, `HTTP_*`, …). Requests run on a pool of worker threads, each
in a fresh engine, with a per-worker opcache. `goro-httpd --help` lists the
options.

[samples/wordpress](samples/wordpress/) runs a stock WordPress on it with
SQLite (the official SQLite Database Integration plugin over goro's PDO
SQLite) and has an end-to-end smoke test: installer, login, dashboard,
pretty permalinks, publishing through the REST API, comments, uploads with
thumbnails and HTTPS.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the detailed design document.

Workspace crates:
- `goro-parser` - Hand-written lexer and recursive descent parser
- `goro-core` - Bytecode compiler and register-based VM
- `goro-vfs` - Virtual filesystem abstraction
- `goro-sapi` - Server API trait and CLI implementation
- `goro-ext-standard` - PHP standard library (streams, sockets, filters, locale, …)
- `goro-ext-date` - Date/time extension (DateTime, DateInterval, etc.)
- `goro-ext-json` - JSON extension (json_encode/decode)
- `goro-ext-ctype` - Character type extension
- `goro-ext-hash` - Hash extension (all php-src algorithms, HMAC/HKDF/PBKDF2)
- `goro-ext-reflection` - Reflection extension (stub; most logic lives in `goro-core::reflection`)
- `goro-ext-spl` - SPL extension (autoload helpers; SPL classes are compiled into `goro-core`)
- `goro-ext-pdo` - PDO (ext/pdo) and the MySQL and PostgreSQL drivers (ext/pdo_mysql, ext/pdo_pgsql)
- `goro-ext-pgsql` - ext/pgsql over `libpq`, a pure-Rust port of PostgreSQL's client library
- `goro-ext-soap` - SOAP client and server (ext/soap)
- `goro-ext-gd` - GD image functions (ext/gd) on `goro-gd`, a pure Rust port of the bundled libgd
- `goro-ext-xsl` - XSLTProcessor (ext/xsl): XSLT 1.0, XPath 1.0 and EXSLT, a port of libxslt/libexslt
- `goro-ext-sysvipc` - shmop, sysvsem, sysvshm and sysvmsg
- `goro-ext-dba` - ext/dba with the cdb, cdb_make, inifile and flatfile handlers
- `goro-ext-readline` - ext/readline (a GNU readline emulation) and the CLI shell's completion
- `goro-sqlite` - a pure Rust port of SQLite 3.54.1
- `goro-ext-sqlite3` - ext/sqlite3 on `goro-sqlite` (pdo_sqlite lives in `goro-ext-pdo`)
- `goro-tidy` - a pure Rust port of tidy-html5 5.8.0 (libtidy)
- `goro-ext-tidy` - ext/tidy on `goro-tidy`
- `goro-ext-opcache` - Zend OPcache (INI, opcache_*() API, preloading), over goro-core's script cache
- `goro-ldap` - a pure-Rust LDAP client modelled on OpenLDAP's libldap/liblber (BER, LDAPv3, URL/DN, filters, controls, TLS over purecrypto)
- `goro-ext-ldap` - ext/ldap (LDAP\Connection, LDAP\Result, LDAP\ResultEntry) on `goro-ldap`
- `goro-ext-uuid` - PECL uuid (uuid_create() & co., libuuid semantics)
- `goro-ext-mailparse` - PECL mailparse (mailparse_msg_*(), mailparse_rfc822_parse_addresses(), mimemessage)
- `goro-imagick` - a pure-Rust image engine with ImageMagick 7 MagickWand semantics, on the OxideAV crates
- `goro-ext-imagick` - PECL imagick (Imagick, ImagickDraw, ImagickPixel, ImagickPixelIterator, ImagickKernel) on `goro-imagick`
- `goro-ext-{bz2,calendar,curl,ftp,gettext,gmp,mbstring,mysqli,openssl,pcntl,posix,session,sockets,sodium,xml,zlib}` - misc extensions
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Pure Rust**: no C FFI dependencies; ports of the reference C code where exact behaviour matters
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
