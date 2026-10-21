# goro-rs

A PHP 8.5 implementation in Rust.

## Status

Active development, aiming at full PHP 8.5 parity. The language core (OOP, traits, enums,
closures, generators, fibers, property hooks, lazy objects, typed properties and references,
match, readonly, asymmetric visibility, pipe operator) and a large part of the standard
extensions run the official php-src test suite.

**php-src 8.5.4 test suite** (full run, master `92b69e9d`, 2026-10-22):

| Metric | Count |
|---|---|
| Total tests | 21455 |
| Pass | 13103 (61.1% of all, **86.9% of runnable**) |
| Fail | 1938 |
| Skip | 6376 (mostly extensions that are not implemented: intl, gd, soap, phar, pdo, …) |
| Error | 38 |

CI runs the whole suite in 8 parallel shards on every push (`GORO_PHPT_SHARD=i/n`), plus
`cargo fmt --check` and `cargo clippy -D warnings`.

**Pass rates by directory** (pass / run, skips excluded):

| Area | Pass | Run | Rate |
|---|---|---|---|
| Zend/tests | 4112 | 5161 | 79.7% |
| tests/ (lang, classes, basic, …) | 749 | 826 | 90.7% |
| ext/standard | 3021 | 3281 | 92.1% |
| ext/spl | 711 | 775 | 91.7% |
| ext/date | 673 | 676 | 99.6% |
| ext/reflection | 403 | 485 | 83.1% |
| ext/dom (incl. HTML5 `Dom\HTMLDocument`) | 819 | 843 | 97.2% |
| ext/simplexml / xml / xmlreader / xmlwriter | 296 | 316 | 93.7% |
| ext/mbstring | 388 | 414 | 93.7% |
| ext/openssl (incl. TLS streams) | 205 | 212 | 96.7% |
| ext/mysqli (embedded MySQL fixture) | 404 | 425 | 95.1% |
| ext/session | 254 | 257 | 98.8% |
| ext/json | 79 | 87 | 90.8% |
| ext/gmp | 91 | 98 | 92.9% |
| ext/ctype / ext/uri | 166 | 166 | 100% |
| ext/opcache (behavioural tests) | 211 | 292 | 72.3% |

Zend highlights: type_declarations 83.5%, traits 90.0%, enum 86.6%, generators 80.4%,
closures 82.2%, lazy_objects 88.3%, property_hooks 87.0%, fibers 89.7%, gc 88.2%,
attributes 82.1%. (Type declarations, pcre, hash, random and zlib have improved further
since this run.)

*The test runner supports recursive directories, single files, SKIPIF/INI/ENV/ARGS sections,
EXPECTF/EXPECTREGEX matching, per-test timeouts and an embedded MySQL fixture for ext/mysqli.*

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
  mysqli (mysqlnd port), gmp, sockets, curl, bz2, tokenizer.

### Not Yet Implemented
- intl, gd, soap, phar, pdo_*, ffi, ldap, pgsql, sqlite3, exif, zip, xsl, iconv (mostly)
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
- `goro-ext-{bz2,curl,gmp,mbstring,mysqli,openssl,session,sockets,xml,zlib}` - misc extensions
- `goro-phpt` - PHPT test file runner

## Design Goals

- **Performance**: Zero-cost abstractions, minimal allocations
- **Completeness**: Target 100% PHP 8.5.4 test suite compatibility
- **Pure Rust**: no C FFI dependencies; ports of the reference C code where exact behaviour matters
- **Security**: Fully scopeable file/network access via VFS
- **Modularity**: SAPIs and extensions are opt-in at compile time
