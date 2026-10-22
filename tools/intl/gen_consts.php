<?php
// Usage: php tools/intl/gen_consts.php consts.tsv > crates/goro-ext-intl/src/consts.rs
// Generate crates/goro-ext-intl/src/consts.rs: the values of ext/intl's
// constants (global and class), in stub declaration order. Values come from
// a php build with ICU (dump of dumpconsts.php) plus overrides for constants
// that build lacks.
$src = '/home/magicaltux/php-src/ext/intl';
$have = [];
foreach (file($argv[1]) as $l) {
    $p = explode("\t", rtrim($l, "\n"));
    if ($p[0] === 'G') $have[$p[1]] = $p[2];
    elseif ($p[0] === 'C') $have[$p[1] . '::' . $p[2]] = $p[3];
}
$over = [
    'INTL_ICU_VERSION' => "'78.1'",
    'INTL_ICU_DATA_VERSION' => "'78.1'",
    'IntlChar::UNICODE_VERSION' => "'17.0'",
    'IntlDateFormatter::PATTERN' => '-2',
    'NumberFormatter::DECIMAL_COMPACT_SHORT' => '14',
    'NumberFormatter::DECIMAL_COMPACT_LONG' => '15',
    'NumberFormatter::CURRENCY_ISO' => '10',
    'NumberFormatter::CURRENCY_PLURAL' => '11',
    'NumberFormatter::CASH_CURRENCY' => '13',
    'NumberFormatter::CURRENCY_STANDARD' => '16',
    'NumberFormatter::ROUND_TOWARD_ZERO' => '2',
    'NumberFormatter::ROUND_AWAY_FROM_ZERO' => '3',
    'NumberFormatter::ROUND_HALFODD' => '8',
    'IntlListFormatter::TYPE_AND' => '0',
    'IntlListFormatter::TYPE_OR' => '1',
    'IntlListFormatter::TYPE_UNITS' => '2',
    'IntlListFormatter::WIDTH_WIDE' => '0',
    'IntlListFormatter::WIDTH_SHORT' => '1',
    'IntlListFormatter::WIDTH_NARROW' => '2',
    'Spoofchecker::IGNORE_SPACE' => '1',
    'Spoofchecker::CASE_INSENSITIVE' => '2',
    'Spoofchecker::ADD_CASE_MAPPINGS' => '4',
    'Spoofchecker::SIMPLE_CASE_INSENSITIVE' => '6',
    'IntlChar::PROPERTY_IDS_UNARY_OPERATOR' => '72',
    'IntlChar::PROPERTY_ID_COMPAT_MATH_START' => '73',
    'IntlChar::PROPERTY_ID_COMPAT_MATH_CONTINUE' => '74',
];
$global = [];
$classes = [];
$files = [];
$it = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($src, FilesystemIterator::SKIP_DOTS));
foreach ($it as $f) {
    $p = $f->getPathname();
    if (str_ends_with($p, '.stub.php') && !str_contains($p, '/tests/')) $files[] = $p;
}
sort($files);
foreach ($files as $p) {
    $class = null;
    $keep = [true];
    foreach (explode("\n", file_get_contents($p)) as $line) {
        $t = trim($line);
        if (preg_match('/^#\s*if\s+U_ICU_VERSION_MAJOR_NUM\s*>=\s*(\d+)/', $t, $m)) { $keep[] = 78 >= (int)$m[1]; continue; }
        if (preg_match('/^#\s*if/', $t)) { $keep[] = true; continue; }
        if (preg_match('/^#\s*else/', $t)) { $keep[count($keep) - 1] = !$keep[count($keep) - 1]; continue; }
        if (preg_match('/^#\s*endif/', $t)) { array_pop($keep); continue; }
        if (in_array(false, $keep, true)) continue;
        if (preg_match('/^\s*(?:final\s+|abstract\s+)*(?:class|interface)\s+(\w+)/', $line, $m)) $class = $m[1];
        if (preg_match('/^const\s+(\w+)\s*=/', $t, $m)) {
            $global[] = $m[1];
        } elseif (preg_match('/^(?:public\s+)?const\s+(?:\w+\s+)?(\w+)\s*=/', $t, $m) && $class) {
            $classes[$class][] = $m[1];
        }
    }
}
function val(string $k): string {
    global $have, $over;
    $v = $over[$k] ?? $have[$k] ?? null;
    if ($v === null) { fwrite(STDERR, "no value for $k\n"); return 'V::I(0)'; }
    if ($v === 'NULL') return 'V::N';
    if ($v[0] === "'") return 'V::S(' . json_encode(trim($v, "'")) . ')';
    if (str_contains($v, '.')) return 'V::D(' . $v . ')';
    return 'V::I(' . $v . ')';
}
echo "//! The values of ext/intl's constants, in stub declaration order\n";
echo "//! (generated from a PHP build with ICU, plus the constants it lacks).\n\n";
echo "#[derive(Clone, Copy, Debug)]\npub enum V {\n    I(i64),\n    D(f64),\n    S(&'static str),\n    N,\n}\n\n";
echo "pub static GLOBAL: &[(&str, V)] = &[\n";
foreach ($global as $g) echo "    (\"$g\", " . val($g) . "),\n";
echo "];\n\n";
echo "pub static CLASSES: &[(&str, &[(&str, V)])] = &[\n";
foreach ($classes as $c => $list) {
    echo "    (\n        \"$c\",\n        &[\n";
    foreach ($list as $k) echo "            (\"$k\", " . val("$c::$k") . "),\n";
    echo "        ],\n    ),\n";
}
echo "];\n";
