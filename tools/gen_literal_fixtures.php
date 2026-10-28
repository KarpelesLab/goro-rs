<?php
// Captures the values PHP 8.5 gives to number and string literals, for the
// literal decoding tests of crates/goro-syntax.
//
// Usage (from the repository root):
//   /tmp/claude-1000/bin/php85 tools/gen_literal_fixtures.php
//
// Input: crates/goro-syntax/tests/data/literal_cases.txt, one literal per
// line, with stripcslashes() escapes for line breaks (\n, \r, \t; "\\" is
// a backslash).
// Output: crates/goro-syntax/tests/data/literal_expected.txt, one line per
// case: "int N", "float <IEEE bits as hex>", "string <hex bytes>" or
// "error <message>".

$dir = __DIR__ . '/../crates/goro-syntax/tests/data';
$out = [];
foreach (file("$dir/literal_cases.txt", FILE_IGNORE_NEW_LINES) as $case) {
    $code = stripcslashes($case);
    try {
        $v = @eval("return $code\n;");
        if (is_int($v)) {
            $out[] = "int $v";
        } elseif (is_float($v)) {
            $out[] = "float " . bin2hex(pack('E', $v));
        } else {
            $out[] = "string " . bin2hex($v);
        }
    } catch (ParseError $e) {
        $out[] = "error " . $e->getMessage();
    }
}
file_put_contents("$dir/literal_expected.txt", implode("\n", $out) . "\n");
