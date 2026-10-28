# goro-rs

A clean-room implementation of PHP 8.5 in Rust.

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

## Status

Fresh start. The previous code base is kept on the `old` branch for the
record. Because it was written with php-src as a reference, it is out of
bounds for this implementation (see [CLEANROOM.md](CLEANROOM.md)).
