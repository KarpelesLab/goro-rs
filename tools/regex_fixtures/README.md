# goro-regex fixtures

Expected results for `crates/goro-regex/tests/` recorded from the reference
PHP 8.5 binary (PCRE2 10.46), used as a black box. Each run is one batched
php85 invocation.

- `errs.php` prints compile errors (message and offset) for erroneous patterns.
- `extract.php <php-tests> <out>` collects the literal (pattern, subject)
  pairs of `preg_*` calls in php-src's `.phpt` files (tests only).
- `cases.php` is the hand-written list of feature cases.
- `gen.php cases.php extracted.txt errs.out` runs `preg_match_all()` with
  `PREG_SET_ORDER | PREG_OFFSET_CAPTURE` on every case and prints one line
  per case: hex(pattern), hex(subject), and the result (`ERR <warning>`,
  `FALSE <preg_last_error()>`, or `N <count>` followed by the group offsets
  of each match).

Regenerate with:

```sh
php85 errs.php > errs.out
php85 extract.php /tmp/claude-1000/php-tests extracted.txt
php85 -d pcre.jit=0 gen.php cases.php extracted.txt errs.out \
    > ../../crates/goro-regex/tests/fixtures/php85_match_all.txt
```
