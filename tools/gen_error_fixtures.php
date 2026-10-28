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
// case: "OK", "<line>\t<message>" for the ParseError that
// token_get_all(..., TOKEN_PARSE) throws (the parser only, no compiler),
// or "FATAL\t<line>\t<message>" for a fatal error raised while parsing.
// Each case runs in a forked child, as a fatal error ends the process.

$dir = __DIR__ . '/../crates/goro-syntax/tests/data';
$tmp = tempnam(sys_get_temp_dir(), 'gef');
$out = [];
foreach (file("$dir/error_cases.txt", FILE_IGNORE_NEW_LINES) as $case) {
    $code = "<?php " . stripcslashes($case);
    file_put_contents($tmp, '');
    $pid = pcntl_fork();
    if ($pid === 0) {
        ini_set('display_errors', '0');
        error_reporting(E_ALL);
        register_shutdown_function(function () use ($tmp) {
            $e = error_get_last();
            if ($e !== null && in_array($e['type'], [E_ERROR, E_COMPILE_ERROR, E_CORE_ERROR, E_PARSE], true)
                && file_get_contents($tmp) === '') {
                file_put_contents($tmp, "FATAL\t" . $e['line'] . "\t" . str_replace(["\\", "\n"], ["\\\\", "\\n"], $e['message']));
            }
        });
        try {
            token_get_all($code, TOKEN_PARSE);
            file_put_contents($tmp, "OK");
        } catch (ParseError $e) {
            file_put_contents($tmp, $e->getLine() . "\t" . str_replace(["\\", "\n"], ["\\\\", "\\n"], $e->getMessage()));
        }
        exit(0);
    }
    pcntl_waitpid($pid, $status);
    $out[] = file_get_contents($tmp);
}
unlink($tmp);
file_put_contents("$dir/error_expected.txt", implode("\n", $out) . "\n");
