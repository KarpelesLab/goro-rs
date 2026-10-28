# goro-rs

A clean-room implementation of PHP 8.5 in Rust, written from scratch.

goro-rs is the Rust counterpart of [goro](https://github.com/KarpelesLab/goro),
a clean-room implementation of PHP in Go. It shares goro's clean-room approach,
not its code: goro-rs is written from scratch, not translated from the Go
version. PHP is a well-documented language, and the manual and the test suite
are enough to build it.

## Goals

1. **PHP parity**: programs behave as they do on PHP 8.5, including output,
   warnings, errors and exceptions.
2. **100% of php-src's tests pass.** php-src's `.phpt` suite is the
   compliance target: every test that runs on PHP 8.5 on Linux must pass on
   goro.
3. **Performance at least equal to PHP's** on the same machine, measured
   against a reference PHP 8.5 binary for both CPU time and memory.
4. **Clean room.** goro is written only from PHP's documentation, public
   specifications, the test suite used as a black box, and the observed
   behaviour of a reference PHP binary. No code is written from php-src or
   from the source of the libraries PHP uses. See [CLEANROOM.md](CLEANROOM.md).

## Constraints

- Pure Rust. No C code and no FFI to C libraries (the `libc` crate for system
  calls is fine).

## Running php-src's tests

`goro --test` runs `.phpt` files (the `goro-phpt` crate):

```sh
goro --test -q -j 8 --copy-to /tmp/phpt-run --results new.txt /path/to/php-tests
goro --test --php /usr/bin/php -v Zend/tests/add_001.phpt   # any PHP binary
goro --test-compare base.txt new.txt     # tests that went from PASS to anything else
goro --test --guard base.txt ...         # the same check at the end of a run
```

Each test runs the binary under test as a subprocess with `-n` and the ini
defaults listed (with their reasons) in `crates/goro-phpt/src/runner.rs`
(`DEFAULT_INI`). The working directory is the test-suite root. `--copy-to`
runs from a copy of that root, so files the tests leave behind don't touch
the checkout. Tests that need a web SAPI (CGI, GET, POST, ...) or phpdbg are
reported as SKIP for now.

## Status

Fresh start. The previous code base is kept on the `old` branch for the
record. Because it was written with php-src as a reference, it is out of
bounds for this implementation (see [CLEANROOM.md](CLEANROOM.md)).

- `goro-runtime`: PHP values (16-byte `Value`, byte strings, copy-on-write
  ordered arrays, references, objects, destructor queue, cycle collector),
  conversions, comparisons and arithmetic, checked against PHP 8.5 output.
- `goro-phpt`: the `.phpt` runner behind `goro --test` (all test sections).
- `goro-ext-standard`: printf family, string, math, sorting, HTML
  entities (WHATWG table), URL and encoding functions.
- `goro-ext-date`: date/time library with tzdata 2026e compiled from the
  IANA database: parsing (`strtotime`, `DateTime::createFromFormat`),
  formatting, intervals, periods and sunrise/sunset, checked on ~33k PHP 8.5
  cases.
- `goro-charset`: mbstring's 79 encodings (tables generated from Unicode,
  WHATWG and UCD 17 data), the `mb_*` string operations, case conversion,
  kana conversion, MIME headers and `mb_detect_encoding` scoring; iconv
  with 247 charsets, `//TRANSLIT` and `//IGNORE`. Checked against PHP 8.5
  fixtures for every 1–2 byte input and all of Unicode.
- `goro-syntax`: lexer, parser and AST for PHP 8.5. The token stream
  matches `token_get_all()`; every `--FILE--` section of the php-src
  tests that PHP accepts parses, and the in-file parse errors the tests
  expect come out with PHP's message and line (`tools/syntax-corpus.sh`).
- `goro-regex`: PCRE2 10.46-compatible regex engine for ext/pcre (syntax,
  PHP modifiers, compile error messages and offsets, backtrack/recursion
  limit and JIT stack accounting, Unicode 16.0), checked against PHP 8.5 on
  ~220k fixture cases (`tools/regex_fixtures/`).
- `goro-ext-hash`: the hash extension on bytes: all 60 `hash_algos()`
  algorithms with streaming contexts, HMAC, HKDF, PBKDF2, the `seed` and
  `secret` options, and `HashContext` serialization in PHP 8.5's format.
  Checked against the specifications' vectors, the ext/hash test cases and
  a 10.8k-line PHP 8.5 fixture; at or above PHP's throughput for the
  common digests (SHA extensions, PCLMULQDQ CRC folding, SIMD XXH3).
- `goro-ext-json`: `json_encode`/`json_decode`/`json_validate` over
  runtime values with every `JSON_*` flag, PHP's error codes and their
  precedence, recursion guards across nested calls, `JsonSerializable`
  and enums through an engine hook trait. Checked against a 33.7k-line
  PHP 8.5 fixture; 1.25-7.6x PHP's throughput on multi-MB documents
  (iterative, so nesting depth is bounded by `$depth` only).
- `goro-ext-random`: ext/random on plain values: the Mt19937 (both
  modes), PcgOneseq128XslRr64, Xoshiro256StarStar and Secure engines with
  their seeding, jumps and serialized states, user engines through a trait,
  every `Random\Randomizer` method (γ-section `getFloat()` included), and
  `mt_rand`/`rand`/`shuffle`/`str_shuffle`/`array_rand`/`random_int`/
  `random_bytes`/`lcg_value`. Seeded sequences, randomness consumption and
  error messages match PHP 8.5 on ~6.5k fixture cases; library-level
  times are below PHP 8.5's (`examples/random_bench.rs`).
- `goro-ext-zlib`: the zlib extension on bytes: `gz*`/`zlib_*` one-shot
  functions with PHP's buffer rounds and warnings, `deflate_*`/`inflate_*`
  contexts (flush modes, dictionaries, statuses), the gzip file layer under
  `gzopen()` and the `zlib.*` filters. Its deflate encoder produces the same
  bytes as PHP 8.5 for every level, strategy, memory level, window and flush
  pattern tried (~3.6k compressed outputs, ~3.6k behaviour cases recorded
  from PHP 8.5); compression at 1.0-1.3x and decompression at 1.3-1.5x
  PHP's throughput.
