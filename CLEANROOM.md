# Clean-room policy

goro-rs is a clean-room implementation of PHP in Rust. Its code is written from
PHP's documented and observable behaviour only, never from the source code of
PHP or of the libraries PHP uses.

This policy applies to everyone who contributes, human or AI agent.

## Allowed sources

- **The PHP manual** (php.net/manual) and other published PHP documentation.
- **PHP RFCs** (wiki.php.net/rfc) and the published language specification.
- **Public standards and specifications**: RFCs, Unicode/CLDR data and
  algorithms, W3C/WHATWG specs, POSIX, format specifications (PNG, ZIP,
  DEFLATE, …).
- **The `.phpt` test files of php-src**, used only as tests: they are run
  against goro as a black box, and their `--EXPECT--` sections are the
  required output. Tests are read to learn what behaviour is required, never
  to infer how php-src implements it. Tests are fetched from php-src at test
  time and are not copied into this repository.
- **A reference PHP binary used as a black box**: running PHP scripts and
  observing their output, errors, timing and resource use.
- **Crates from crates.io** used as dependencies through their public APIs,
  under their own licences.

## Forbidden sources

- **php-src source code**: C and header files (`Zend/`, `main/`, `ext/*/*.c`,
  `*.h`), the VM definition files (`zend_vm_def.h`, …), the grammar
  (`zend_language_parser.y`, `zend_language_scanner.l`), the `*.stub.php`
  files and their generated `*_arginfo.h`, and any other non-test file of the
  php-src tree. That includes generating code or tables from these files.
- **Source code of the libraries PHP bundles or links**: timelib, PCRE2,
  libmbfl, oniguruma, libxml2, libxslt, libgd, SQLite, libzip, libmagic,
  OpenSSL, libcurl, ICU, lexbor, uriparser, tidy-html5, libpq, and the rest.
  Implement these from their documentation and the relevant specifications.
- **Source code of PECL extensions and other PHP implementations.**
- **goro, the Go implementation.** It is clean-room too, but goro-rs is
  written from scratch: design and code are not translated from it.
- **The `old` branch of this repository.** It was written with php-src and
  library source as a reference, so it is derived code. Nothing may be copied,
  ported or consulted from it.

## Practices

- A behaviour question is answered by writing a PHP script and running it on
  the reference PHP binary, by reading the manual, or by reading a test's
  expected output. It is never answered by opening an implementation.
- Commit messages and code comments cite where a behaviour came from (the
  manual page, the RFC, the test, or "observed on PHP 8.5"), never a php-src
  function or file.
- Tables (character sets, entities, time zones, Unicode properties) are
  generated from the public standards that define them, not extracted from
  php-src or library sources.
- A contributor who has studied a forbidden source in detail should not write
  the matching part of goro.
