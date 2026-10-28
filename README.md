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

- `goro-syntax`: lexer, parser and AST for PHP 8.5. The token stream
  matches `token_get_all()`; every `--FILE--` section of the php-src
  tests that PHP accepts parses, and the in-file parse errors the tests
  expect come out with PHP's message and line (`tools/syntax-corpus.sh`).
- `goro-regex`: PCRE2 10.46-compatible regex engine for ext/pcre (syntax,
  PHP modifiers, compile error messages and offsets, backtrack/recursion
  limit and JIT stack accounting, Unicode 16.0), checked against PHP 8.5 on
  ~220k fixture cases (`tools/regex_fixtures/`).
