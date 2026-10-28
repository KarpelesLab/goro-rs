<?php
// Captures token_get_all() output from the reference PHP binary for the
// lexer test cases of crates/goro-syntax.
//
// Usage (from the repository root):
//   /tmp/claude-1000/bin/php85 tools/gen_lexer_fixtures.php
//
// Input: crates/goro-syntax/tests/data/lexer_cases.txt, cases separated by a
// line holding only "%%%%". The newline before a separator is not part of
// the case. Escapes in a case: "\n" is literal unless the case starts with
// "#escaped#", then PHP double-quote escapes are decoded.
// Output: crates/goro-syntax/tests/data/lexer_expected.txt, one line per
// token, "id line hex(text)", cases separated by "%%%%".

$dir = __DIR__ . '/../crates/goro-syntax/tests/data';
$cases = explode("\n%%%%\n", rtrim(file_get_contents("$dir/lexer_cases.txt"), "\n"));
$out = [];
foreach ($cases as $case) {
    if (str_starts_with($case, "#escaped#")) {
        $case = stripcslashes(substr($case, 9));
    }
    $lines = [];
    foreach (token_get_all($case) as $t) {
        if (is_array($t)) {
            $lines[] = $t[0] . ' ' . $t[2] . ' ' . bin2hex($t[1]);
        } else {
            $lines[] = '0 0 ' . bin2hex($t);
        }
    }
    $out[] = implode("\n", $lines);
}
file_put_contents("$dir/lexer_expected.txt", implode("\n%%%%\n", $out) . "\n");
