<?php
// Records the mb_encode_mimeheader()/mb_decode_mimeheader() calls with
// literal arguments found in the php-src test files, in the format of
// mb_ops.php. Usage: php mb_mime.php PHPT_DIR > mb_mime.jsonl
error_reporting(E_ALL & ~E_DEPRECATED);
ini_set('display_errors', '0');

function enc($v) {
    if (is_string($v)) return preg_match('/^[\x20-\x7e]*$/', $v) ? ['s' => $v] : ['h' => bin2hex($v)];
    if (is_array($v)) return array_map('enc', $v);
    return $v;
}

// The argument list of the call starting at $i (just after its "("), or null.
function args_at($src, $i) {
    $depth = 1;
    $n = strlen($src);
    for ($j = $i; $j < $n; $j++) {
        $c = $src[$j];
        if ($c === '"' || $c === "'") {
            for ($j++; $j < $n && $src[$j] !== $c; $j++) {
                if ($src[$j] === '\\') $j++;
            }
            continue;
        }
        if ($c === '(') $depth++;
        if ($c === ')' && --$depth === 0) return substr($src, $i, $j - $i);
    }
    return null;
}

function rec($f, ...$args) {
    try {
        $r = enc($f(...$args));
    } catch (Throwable $e) {
        $r = ['e' => get_class($e), 'm' => $e->getMessage()];
    }
    echo json_encode([$f, enc($args), $r], JSON_UNESCAPED_SLASHES), "\n";
}

// Layout: line lengths, indents, spaces, words too long for a line.
mb_internal_encoding('UTF-8');
echo json_encode(['internal', enc(['UTF-8']), null]), "\n";
foreach (["S ", "S  ", "ab ab ", " S", "  ", "a", "ab  ab  ", " a b ", "a   b"] as $s) {
    foreach (["\r\n", "", "X"] as $lf) foreach ([0, 70, 73] as $ind) rec('mb_encode_mimeheader', $s, 'ASCII', 'Q', $lf, $ind);
}
foreach ([" é", "  é", "a é", "a  é", "a   é b", "é a", "é  a ", "a é  ", "a\té", "\té", "a=b c?d e_f", "a\nb", "a\rb c", "a\x7fb"] as $s) {
    rec('mb_encode_mimeheader', $s, 'UTF-8', 'B');
    rec('mb_encode_mimeheader', $s, 'UTF-8', 'Q');
}
for ($i = 40; $i <= 80; $i++) {
    rec('mb_encode_mimeheader', "?", 'ASCII', 'Q', "\n", $i);
    rec('mb_encode_mimeheader', "é", 'UTF-8', 'B', "\n", $i);
    rec('mb_encode_mimeheader', "ab é", 'UTF-8', 'B', "\n", $i);
    rec('mb_encode_mimeheader', "ab cd", 'UTF-8', 'B', "\n", $i);
}
for ($n = 60; $n <= 85; $n++) {
    rec('mb_encode_mimeheader', str_repeat("x", $n), 'UTF-8', 'B');
    rec('mb_encode_mimeheader', str_repeat("x", $n) . " ", 'UTF-8', 'B');
    rec('mb_encode_mimeheader', "ab " . str_repeat("x", $n), 'UTF-8', 'B');
    rec('mb_encode_mimeheader', "ab " . str_repeat("x", $n) . " cd", 'UTF-8', 'Q');
    rec('mb_encode_mimeheader', str_repeat(" ", $n) . "ab", 'UTF-8', 'Q');
    rec('mb_encode_mimeheader', "a" . str_repeat(" ", $n) . "ab", 'UTF-8', 'B');
}
for ($n = 20; $n <= 50; $n++) rec('mb_encode_mimeheader', str_repeat("ab ", $n), 'UTF-8', 'Q');
foreach (["2 ", "22 ", "222 ", "2222 ", "2 2 ", "2 2 2 ", "2    "] as $p) {
    for ($n = 82; $n <= 93; $n++) rec('mb_encode_mimeheader', $p . str_repeat("x", $n), 'UTF-8', 'B', "\r\n", $n & 7);
}
foreach ([1, 10, 30, 60, 70, 74, 75, 80] as $n) {
    foreach (['B', 'Q'] as $t) {
        rec('mb_encode_mimeheader', str_repeat("é", $n), 'UTF-8', $t);
        rec('mb_encode_mimeheader', "ab cd " . str_repeat("é", $n) . " ef gh", 'UTF-8', $t);
        rec('mb_encode_mimeheader', str_repeat("日本", $n), 'ISO-2022-JP', $t);
        rec('mb_encode_mimeheader', "x " . str_repeat("日本", $n), 'SJIS', $t, "\n", 30);
    }
}
rec('mb_encode_mimeheader', "Subject: é and more words here to make it long enough to wrap around the line limit of seventy-four", 'UTF-8', 'B');
rec('mb_encode_mimeheader', "Subject: é and more words here to make it long enough to wrap around the line limit of seventy-four", 'UTF-8', 'Q');

foreach (glob($argv[1] . '/mb_*mimeheader*.phpt') as $file) {
    $t = file_get_contents($file);
    if (!preg_match('/--FILE--\n(.*?)\n--EXPECT/s', $t, $m)) continue;
    $src = $m[1];
    $internal = preg_match('/mb_internal_encoding\([\'"]([^\'"]+)[\'"]\)/', $src, $im) ? $im[1] : 'UTF-8';
    mb_internal_encoding($internal);
    echo json_encode(['internal', enc([$internal]), null]), "\n";
    preg_match_all('/mb_(en|de)code_mimeheader\(/', $src, $calls, PREG_OFFSET_CAPTURE);
    foreach ($calls[0] as [$name, $off]) {
        $a = args_at($src, $off + strlen($name));
        if ($a === null || preg_match('/\$/', $a)) continue;
        try {
            $args = eval("return [$a];");
        } catch (Throwable $e) {
            continue;
        }
        if (!$args) continue;
        $f = rtrim($name, '(');
        try {
            $r = enc($f(...$args));
        } catch (Throwable $e) {
            $r = ['e' => get_class($e), 'm' => $e->getMessage()];
        }
        echo json_encode([$f, enc($args), $r], JSON_UNESCAPED_SLASHES), "\n";
    }
}
