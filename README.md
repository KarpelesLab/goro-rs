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

php-src test suite (8.5.4), share of the tests that run: **89.9%** overall
(13,640 passing), as of 2026-10-29.
The extension libraries below are wired into the engine; ext/random,
mbstring, iconv, zlib, bcmath, gmp, filter, sodium, uri, openssl, intl and
the XML extensions each pass 87-100% of their tests that run.

- `goro-engine` and the `goro` binary: compiler and VM running PHP 8.5
  scripts (classes, enums, closures, generators, fibers, attributes,
  references, exceptions, output buffering, streams, SPL, Reflection,
  serialization). Speed is currently 1.3–4.7x PHP's time on benchmarks.
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
- `goro-ext-bcmath`: ext/bcmath on byte strings: every `bc*()` function
  (all `RoundingMode`s, `bcdivmod`), `BcMath\Number` (scale rules,
  operators, comparison, serialization), PHP's error messages and their
  order, and the `bcmath.scale` ini quantity parsing. Checked against a
  25.8k-case PHP 8.5 fixture. SWAR digit arithmetic on the ASCII strings,
  base-10⁸ limbs (Karatsuba, Knuth division) and puremp for large
  divisions and roots: faster than PHP on 20-digit operands and 4-500x
  faster at 10k-100k digits.
- `goro-ext-gmp`: ext/gmp on puremp: every `gmp_*()` function (PHP's
  integer-string parsing in bases 0/2-62, import/export word orders,
  rounding modes, GMP's results for inexact `gmp_divexact()`, its
  prime/probable-prime answers, its seeded `gmp_random_*` streams) and the
  `GMP` class (operators, comparisons, casts, serialization, constructor),
  with PHP's argument coercion, deprecations and error messages. Checked
  against a 52.8k-case PHP 8.5 fixture. Single-limb fast paths
  (Montgomery `powm`, Baillie-PSW, binary GCD) put small operations at or
  below PHP's times; large operands run at 1.5-3.5x GMP's times.
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
- `goro-ext-filter`: ext/filter on runtime values: every filter of
  `filter_list()` (the int, float, bool, regexp, domain, URL, e-mail, IP
  and MAC validators with all their flags and options, the sanitizers,
  FILTER_CALLBACK through a host hook), `filter_var`/`filter_var_array`/
  `filter_input`/`filter_input_array`/`filter_has_var` with PHP 8.5's
  FILTER_THROW_ON_FAILURE, input sources as a host trait, and
  `filter.default` for the SAPI. Checked against a 224k-line PHP 8.5
  fixture (~320k cases: every byte at every position of sample values, the
  IP range boundaries, every Unicode letter and number for
  FILTER_FLAG_EMAIL_UNICODE); common validators take 7-80 ns, 1.2-5.7x
  faster than PHP 8.5's filter_var() (`examples/filter_bench.rs`).
- `goro-ext-sodium`: ext/sodium: all 110 `sodium_*()` functions (AEGIS
  included) and the `SODIUM_*` constants, with PHP's argument checks,
  messages, warnings and state strings (BLAKE2b and secretstream states
  byte-compatible with PHP's). purecrypto for ChaCha20/Poly1305, the AEADs,
  SHA-512, SipHash, scrypt and Ristretto255; Salsa20/XSalsa20 (AVX2 and
  AVX-512), BLAKE2b, Argon2 (AVX2, prefetching, huge pages), X25519 and
  variable-time Ed25519 verification written here. Checked against the
  specifications' vectors, the ext/sodium tests and a 4.7k-case PHP 8.5
  fixture; faster than PHP 8.5 (libsodium 1.0.18) on secretbox, the
  AEADs, generichash, sign/verify, box and Argon2/scrypt
  (`examples/bench.rs` against `tests/fixtures/bench.php`).
- `goro-ext-openssl`: ext/openssl on purecrypto: RSA/DSA/DH/EC/X25519/
  Ed25519/X448/Ed448 keys (PKCS #1/#8, SEC 1, encrypted PEM and PBES2),
  X.509 parsing with PHP's array and text forms, extensions from config
  sections, CSRs, path validation and purposes, PKCS #7/CMS signing,
  verification and encryption with S/MIME, PKCS #12, SPKAC, every cipher
  of `openssl_get_cipher_methods()` (OCB, SIV, GCM-SIV and key wrap
  written here), and TLS streams with the `ssl` context options on a new
  socket layer (`stream_socket_*`, `fsockopen`). DER, PEM and the
  OpenSSL config format are parsed here; error strings, NIDs and names
  follow PHP 8.5's output.
- `goro-ext-uri`: ext/uri as plain values: `Uri\Rfc3986\Uri` (strict
  RFC 3986 parsing, raw and normalized getters, withers, resolution,
  equality) and `Uri\WhatWg\Url` (the WHATWG URL parser with UTS #46 16.0
  IDNA, PHP's validation-error list and contexts, withers sharing the
  error list as PHP does, ASCII/Unicode serializations), the exception
  messages, `__serialize`/`__unserialize`/`__debugInfo` shapes and a
  FILTER_VALIDATE_URL hook. Checked against PHP 8.5 fixtures (13.6k
  RFC 3986 cases, 8.7k WHATWG parses, 5.4k withers, every code point
  through IDNA) and the WPT url, toascii, IdnaTestV2 and setters data.
  Parsing typical URLs takes 25-50 ns (RFC 3986) and 60-145 ns (WHATWG),
  below PHP 8.5's 105-215 ns (`examples/uri_bench.rs`).
- `goro-ext-intl`: ext/intl on the `intl` crate (CLDR 48, Unicode 17;
  `INTL_ICU_VERSION` 78.1): `Locale` (ICU locale IDs and BCP 47 tags,
  display names, canonicalization, RFC 4647 matching), `Collator`,
  `NumberFormatter` (ICU decimal patterns, currencies, rule-based styles,
  parsing), `MessageFormatter` (plural/select/choice, number and date
  arguments), `IntlDateFormatter`, `IntlDatePatternGenerator`,
  `IntlCalendar`/`IntlGregorianCalendar` (with the other CLDR calendars),
  `IntlTimeZone`, break iterators (UAX #29/#14 and rule-based),
  `Transliterator`, `Normalizer`, `grapheme_*()`, UTS #46 IDNA,
  `Spoofchecker`, `UConverter` (goro-charset codecs), `ResourceBundle`
  over `.res` files, `IntlListFormatter`, `IntlChar`, and intl's error
  handling. CLDR data generated by `tools/gen_intl_*.py`; ext/intl tests:
  413 of 421 that run pass.
- `goro-ext-exif`: ext/exif and the image functions of ext/standard on
  bytes: `exif_read_data()` (JPEG APP1, TIFF and HEIF Exif with the IFD0,
  THUMBNAIL, EXIF, GPS, INTEROP, WINXP and maker note sections of ten
  makers, COMPUTED values, PHP's diagnostics and error limits),
  `exif_thumbnail()`, `getimagesize()` for every `IMAGETYPE_*` format
  (SWC through goro-ext-zlib, AVIF/HEIF properties, SVG), `iptcparse()`
  and `iptcembed()`. Checked against PHP 8.5 output on the ext/exif,
  ext/standard/tests/image and ext/gd test images and crafted files;
  ext/exif and ext/standard/tests/image: 132 of 132 tests that run pass.
- `goro-ext-gd`: ext/gd as bundled libgd 2.1 behaves: palette and
  truecolor images with GD's 7-bit alpha and layer effects, color
  allocation/matching, lines (thick, dashed, styled, brushed, tiled,
  antialiased), rectangles, polygons, arcs and pies as GD rasterizes
  them, fills, copy/merge/resize/resample, rotate/scale/crop/affine,
  `imagefilter()`/convolution/gamma, median-cut palette conversion,
  the five built-in fonts and `.gdf` fonts, TrueType/OpenType text
  (oxideav-ttf outlines, own coverage rasterizer), and the PNG, GIF,
  BMP, WBMP, XBM, XPM, TGA, GD and GD2 codecs written here plus JPEG,
  WebP and AVIF through oxideav. Drawing is pixel-exact against the
  tests' reference images; ext/gd: 303 of 308 tests that run pass.
- `goro-xml`: the XML core for ext/dom, simplexml, ext/xml, xmlreader and
  xmlwriter (not yet wired): XML 1.0 + namespaces parser with PHP's libxml
  error codes, messages and positions, DTDs, entities, LIBXML_* options and
  DTD validation; an arena node tree for `DOMDocument` and `Dom\`; legacy
  `saveXML`/`saveHTML` and the `Dom\` (DOM Parsing) serializers; the
  legacy HTML parser of `loadHTML`; a WHATWG HTML5 parser/serializer for
  `Dom\HTMLDocument`; XPath 1.0, CSS selectors, C14N, XInclude; ext/xml's
  event API, XMLReader and XMLWriter. W3C XML conformance suite:
  1946/1974 well-formedness verdicts (the rest are namespace errors libxml
  only warns about); html5lib-tests: 1756/1756 tree construction,
  7016/7016 tokenizer. Fixtures recorded on PHP 8.5 in `tests/fixtures`.
