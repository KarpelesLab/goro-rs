# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development, aiming at full PHP 8.5 parity. The language core (OOP, traits, enums,
closures, generators, fibers, property hooks, lazy objects, typed properties and references,
match, readonly, asymmetric visibility, pipe operator) and a large part of the standard
extensions run the official php-src test suite.

**php-src 8.5.4 test suite** (full run, master `73ce7152`, 2026-10-23):

| Metric | Count |
|---|---|
| Total tests | 21582 (the generic PDO tests run once per driver) |
| Pass | 20007 (92.7% of all, **98.9% of runnable**) |
| Fail / Error | 230 |
| Skip / XFail | 1345 (mostly extensions not implemented: ffi, odbc, snmp, enchant (out of scope), …; plus Windows-only, root-only and huge-memory tests) |

CI runs the whole suite in 8 parallel shards on every push (`GORO_PHPT_SHARD=i/n`), plus
`cargo fmt --check` and `cargo clippy -D warnings`.

**Pass rates by directory** (pass / run, skips excluded):

| Area | Pass | Run | Rate |
|---|---|---|---|
| Zend/tests | 5157 | 5253 | 98.2% |
| tests/ (lang, classes, basic, …) | 822 | 829 | 99.2% |
| ext/standard | 3345 | 3358 | 99.6% |
| ext/spl | 776 | 779 | 99.6% |
| ext/date | 677 | 677 | 100% |
| ext/reflection | 488 | 493 | 99.0% |
| ext/dom (incl. HTML5 `Dom\HTMLDocument`) | 849 | 850 | 99.9% |
| ext/simplexml / xml / xmlreader / xmlwriter / libxml | 339 | 344 | 98.5% |
| ext/soap | 574 | 574 | 100% |
| ext/gd (bundled libgd port; PNG/GIF/JPEG/WebP/AVIF/BMP/TGA/XBM/XPM/WBMP/GD2, FreeType) | 308 | 308 | 100% |
| ext/xsl (libxslt + libexslt port) | 76 | 76 | 100% |
| ext/mbstring | 415 | 415 | 100% |
| ext/intl (on the `intl` crate) | 416 | 427 | 97.4% |
| ext/iconv (glibc iconv emulation) | 68 | 68 | 100% |
| ext/openssl (incl. TLS streams, on purecrypto) | 208 | 213 | 97.7% |
| ext/sodium (on purecrypto) | 30 | 30 | 100% |
| ext/mysqli (embedded MySQL fixture) | 420 | 420 | 100% |
| ext/pdo (generic tests, run against MySQL, PostgreSQL and SQLite) | 366 | 369 | 99.2% |
| ext/sqlite3 / pdo_sqlite (goro-sqlite, a SQLite 3.54.1 port) | 174 | 175 | 99.4% |
| ext/pdo_mysql / pdo_pgsql | 219 | 219 | 100% |
| ext/pgsql (libpq port, embedded PostgreSQL fixture) | 96 | 96 | 100% |
| ext/ldap (pure-Rust libldap port, in-process LDAP server fixture) | 131 | 131 | 100% |
| ext/session | 256 | 256 | 100% |
| ext/pcre (PCRE2 port) | 155 | 155 | 100% |
| ext/hash / random / zlib / bz2 | 305 | 306 | 99.7% |
| ext/phar (phar, tar, zip formats) | 540 | 542 | 99.6% |
| ext/zip (libzip port) / exif / fileinfo (libmagic port) | 265 | 265 | 100% |
| ext/json | 88 | 88 | 100% |
| ext/bcmath / filter / gmp (bcmath and gmp on puremp) | 379 | 379 | 100% |
| ext/ctype / tokenizer | 102 | 102 | 100% |
| ext/uri (uriparser port + lexbor WHATWG URL parser port) | 309 | 309 | 100% |
| ext/posix / pcntl / calendar / ftp / gettext / shmop / sysv* | 253 | 253 | 100% |
| ext/curl (libcurl emulation) / sockets | 246 | 246 | 100% |
| ext/opcache (script cache, preloading; 59 of the fails expect optimizer opcode dumps) | 806 | 879 | 91.7% |
| ext/zend_test (engine test hooks) | 145 | 148 | 98.0% |
| ext/dba (cdb, cdb_make, inifile, flatfile) / readline | 53 | 53 | 100% |
| ext/tidy (tidy-html5 5.8.0 port) | 50 | 50 | 100% |
| sapi/cli (incl. `php -S` and `php -a`) | 101 | 102 | 99.0% |
| PECL uuid 1.3 (`tests/pecl/uuid`) | 14 | 14 | 100% |


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
  zend_test; and the PECL extension uuid (RFC 9562 UUIDs v1/v3/v4/v5/v6/v7, the API of
  util-linux libuuid).

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
```

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
- `goro-ext-{bz2,calendar,curl,ftp,gettext,gmp,mbstring,mysqli,openssl,pcntl,posix,session,sockets,sodium,xml,zlib}` - misc extensions
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Pure Rust**: no C FFI dependencies; ports of the reference C code where exact behaviour matters
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
