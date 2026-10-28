<?php
// Captures PHP 8.5's parse results for the parser error test cases of
// crates/goro-syntax.
//
// Usage (from the repository root):
//   /tmp/claude-1000/bin/php85 tools/gen_error_fixtures.php
//
// Input: crates/goro-syntax/tests/data/error_cases.txt, one case per line,
// PHP code after "<?php " with stripcslashes() escapes (\n, \t, \\).
// Output: crates/goro-syntax/tests/data/error_expected.txt, one line per
// case: "OK", or "<line>\t<message>" of the ParseError that
// token_get_all(..., TOKEN_PARSE) throws (the parser only, no compiler).

$dir = __DIR__ . '/../crates/goro-syntax/tests/data';
$out = [];
foreach (file("$dir/error_cases.txt", FILE_IGNORE_NEW_LINES) as $case) {
    $code = "<?php " . stripcslashes($case);
    try {
        token_get_all($code, TOKEN_PARSE);
        $out[] = "OK";
    } catch (ParseError $e) {
        $out[] = $e->getLine() . "\t" . str_replace(["\\", "\n"], ["\\\\", "\\n"], $e->getMessage());
    }
}
file_put_contents("$dir/error_expected.txt", implode("\n", $out) . "\n");
