# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development, aiming at full PHP 8.5 parity. The language core (OOP, traits, enums,
closures, generators, fibers, property hooks, lazy objects, typed properties and references,
match, readonly, asymmetric visibility, pipe operator) and a large part of the standard
extensions run the official php-src test suite.

**php-src 8.5.4 test suite** (full run, master `4e8381e2`, 2026-10-22):

| Metric | Count |
|---|---|
| Total tests | 21455 |
| Pass | 15529 (72.4% of all, **96.4% of runnable**) |
| Fail | 556 |
| Skip | 5350 (mostly extensions that are not implemented yet: soap, phar, intl, gd, sqlite, …) |
| Error | 10 |
| XFail / XPass | 9 / 1 |

CI runs the whole suite in 8 parallel shards on every push (`GORO_PHPT_SHARD=i/n`), plus
`cargo fmt --check` and `cargo clippy -D warnings`.

**Pass rates by directory** (pass / run, skips excluded):

| Area | Pass | Run | Rate |
|---|---|---|---|
| Zend/tests | 4850 | 5230 | 92.7% |
| tests/ (lang, classes, basic, …) | 818 | 828 | 98.8% |
| ext/standard | 3251 | 3303 | 98.4% |
| ext/spl | 764 | 775 | 98.6% |
| ext/date | 673 | 676 | 99.6% |
| ext/reflection | 468 | 491 | 95.3% |
| ext/dom (incl. HTML5 `Dom\HTMLDocument`) | 844 | 845 | 99.9% |
| ext/simplexml / xml / xmlreader / xmlwriter / libxml | 336 | 342 | 98.2% |
| ext/mbstring | 415 | 415 | 100% |
| ext/openssl (incl. TLS streams, on purecrypto) | 205 | 211 | 97.2% |
| ext/mysqli (embedded MySQL fixture) | 401 | 419 | 95.7% |
| ext/pdo_mysql (embedded MySQL fixture) | 160 | 160 | 100% |
| ext/pdo (generic PDO tests, run against pdo_mysql) | 122 | 125 | 97.6% |
| ext/session | 255 | 256 | 99.6% |
| ext/pcre (PCRE2 port) | 154 | 154 | 100% |
| ext/hash / ext/random / ext/zlib / ext/bz2 | 304 | 306 | 99.3% |
| ext/json (port of json_encoder.c / scanner / parser) | 88 | 88 | 100% |
| ext/bcmath (libbcmath port, `BcMath\Number`) | 165 | 165 | 100% |
| ext/filter | 116 | 116 | 100% |
| ext/iconv (glibc iconv emulation) | 68 | 68 | 100% |
| ext/gmp | 96 | 98 | 98.0% |
| ext/ctype / ext/uri / ext/tokenizer | 218 | 219 | 99.5% |
| ext/opcache (behavioural tests) | 300 | 303 | 99.0% |
| ext/zend_test (engine test hooks) | 104 | 142 | 73.2% |
| ext/curl (libcurl emulation) | 158 | 158 | 100% |
| ext/sockets | 94 | 95 | 98.9% |
| sapi/cli (incl. `php -S`) | 96 | 96 | 100% |


*The test runner supports recursive directories, single files, SKIPIF/INI/ENV/ARGS sections,
EXPECTF/EXPECTREGEX matching, XFAIL, per-test timeouts and an embedded MySQL fixture for ext/mysqli, ext/pdo_mysql and the generic ext/pdo tests.*

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
  mysqli (mysqlnd port), PDO with pdo_mysql (over the same mysqlnd), gmp, sockets, curl, bz2, tokenizer.

### Not Yet Implemented
- gd, pdo_sqlite/pdo_pgsql/…, ffi, ldap, pgsql, sqlite3, xsl, tidy, sodium (in progress: soap, phar, intl, zip, exif, fileinfo, posix, pcntl, calendar, ftp)
- PCRE JIT (the PCRE2 port is the interpreter only; `PCRE_JIT_SUPPORT` is false)
- Exact opcache optimizer behaviour

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
- `goro-ext-pdo` - PDO (ext/pdo) and the MySQL driver (ext/pdo_mysql)
- `goro-ext-{bz2,curl,gmp,mbstring,mysqli,openssl,session,sockets,xml,zlib}` - misc extensions
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Pure Rust**: no C FFI dependencies; ports of the reference C code where exact behaviour matters
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
