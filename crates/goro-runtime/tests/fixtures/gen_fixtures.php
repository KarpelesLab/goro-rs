<?php
// Fixture generator for goro-runtime's data-driven tests.
//
// Run ONCE against the reference PHP 8.5 binary (black box) and commit the
// output; the Rust tests then check goro against it:
//
//   php85 -d error_reporting=-1 -d display_errors=1 -d log_errors=0 \
//         -d html_errors=0 gen_fixtures.php > php85_fixtures.txt
//
// Values are encoded so the Rust side can decode them exactly:
//   N  null | b:0 b:1 | i:<int> | d:<16 hex digits, IEEE bits> | s:<hex bytes>
//   a:{k=v,k=v}  (keys and values encoded the same way)
//   T:<class>:<message>   a thrown Throwable
// Diagnostics raised while computing a result are listed as
//   <errno>:<message> joined by "|" ("-" when none).

$diags = [];
set_error_handler(function ($no, $str) use (&$diags) {
    $diags[] = "$no:$str";
    return true;
});

function enc($v): string
{
    if ($v === null) return "N";
    if (is_bool($v)) return $v ? "b:1" : "b:0";
    if (is_int($v)) return "i:$v";
    if (is_float($v)) return "d:" . bin2hex(pack('E', $v));
    if (is_string($v)) return "s:" . bin2hex($v);
    if (is_array($v)) {
        $parts = [];
        foreach ($v as $k => $x) $parts[] = enc($k) . "=" . enc($x);
        return "a:{" . implode(",", $parts) . "}";
    }
    return "?";
}

function run(callable $f): string
{
    global $diags;
    $diags = [];
    try {
        $r = enc($f());
    } catch (Throwable $e) {
        $r = "T:" . get_class($e) . ":" . $e->getMessage();
    }
    $d = $diags ? implode("|", $diags) : "-";
    $diags = [];
    return "$d\t$r";
}

// ---------------------------------------------------------------- values
$V = [
    null, true, false,
    0, 1, -1, 42, -42, 255, PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX - 1, 1 << 53,
    0.0, -0.0, 1.0, -1.0, 1.5, -1.5, 0.5, -0.5, 2.5, 0.1, 0.7, 1.9999, -1.9999,
    1e15, 1e16, 1e19, -1e19, 1e100, -1e100, 9.2233720368547758E18,
    -9.2233720368547758E18, 9.2233720368547748E18, 1.8446744073709552E19,
    123456789.123, 1e-10, 4.9e-324, 1.7976931348623157e308,
    INF, -INF, NAN,
    "", " ", "  ", "0", "00", "1", "-1", "+1", "-0", "+0", "-0.0", "0.0",
    " 1", "1 ", " 1 ", "\t\n\r\x0b\x0c1", "1\t\n\r\x0b\x0c", "1\n", "\n1",
    "1.5", ".5", "5.", ".", "-.5", "+.5", "1e3", "1E3", "1e", "1e+", "1e-3",
    "-1e3", "+.5e-3", "1.5e3", "1e+3", "1.e3", ".e3", "e3", "1e3.5",
    "0x1A", "0X1A", "0b11", "0o7", "012", "0012", "1_000", "1,5",
    "abc", "a", "Z", "5 apples", "5apples", "1.5abc", " 1.5 xyz", "1e3abc",
    "  12abc", "12 abc", "1 1",
    "9223372036854775807", "9223372036854775808", "-9223372036854775808",
    "-9223372036854775809", "9999999999999999999", "-9999999999999999999",
    "18446744073709551616", "1e19", "-1e19", "1e1000", "-1e1000",
    "1e-1000", "4.9e-324", "inf", "INF", "nan", "NAN", "infinity",
    "\x001", "1\x00", "0e0", "00001", "+", "-", "+-1", "--1", "1.0",
    "1.50", "100", "  -42  ", "007", "0.1", "1e15", "123456789012345678",
    "1.9999", "-1.9999", "2.5", "-2.5", "1e308", "1.8e308",
    [], [1], [0 => "a"], ["a" => 1, "b" => 2],
];

echo "@values\n";
foreach ($V as $i => $v) echo "$i\t", enc($v), "\n";

echo "@convert\n";
// index  op  diags  result
foreach ($V as $i => $v) {
    echo "$i\tbool\t", run(fn() => (bool)$v), "\n";
    echo "$i\tint\t", run(fn() => (int)$v), "\n";
    echo "$i\tfloat\t", run(fn() => (float)$v), "\n";
    if (!is_array($v)) echo "$i\tstring\t", run(fn() => (string)$v), "\n";
    echo "$i\tis_numeric\t", run(fn() => is_numeric($v)), "\n";
    echo "$i\tarray\t", run(fn() => (array)$v), "\n";
    echo "$i\tkey\t", run(function () use ($v) { $a = []; $a[$v] = 1; return array_key_first($a); }), "\n";
    echo "$i\tneg\t", run(fn() => -$v), "\n";
    echo "$i\tplus\t", run(fn() => +$v), "\n";
    echo "$i\tbitnot\t", run(fn() => ~$v), "\n";
    echo "$i\tnot\t", run(fn() => !$v), "\n";
    echo "$i\tinc\t", run(function () use ($v) { $x = $v; $x++; return $x; }), "\n";
    echo "$i\tdec\t", run(function () use ($v) { $x = $v; $x--; return $x; }), "\n";
}

// ---------------------------------------------------------------- binary ops
$B = [
    null, true, false, 0, 1, -1, 7, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.5,
    -2.5, 1e20, INF, NAN, "", "0", "1", "abc", "5 apples", "1.5", " 7 ",
    "1e3", "0x1A", "-0", "a", "10", "9", [], [1, 2], ["a" => 1],
];
echo "@bvalues\n";
foreach ($B as $i => $v) echo "$i\t", enc($v), "\n";

$ops = [
    '+' => fn($a, $b) => $a + $b,
    '-' => fn($a, $b) => $a - $b,
    '*' => fn($a, $b) => $a * $b,
    '/' => fn($a, $b) => $a / $b,
    '%' => fn($a, $b) => $a % $b,
    '**' => fn($a, $b) => $a ** $b,
    'intdiv' => fn($a, $b) => intdiv($a, $b),
    '.' => fn($a, $b) => $a . $b,
    '&' => fn($a, $b) => $a & $b,
    '|' => fn($a, $b) => $a | $b,
    '^' => fn($a, $b) => $a ^ $b,
    '<<' => fn($a, $b) => $a << $b,
    '>>' => fn($a, $b) => $a >> $b,
    '==' => fn($a, $b) => $a == $b,
    '===' => fn($a, $b) => $a === $b,
    '<=>' => fn($a, $b) => $a <=> $b,
    '<' => fn($a, $b) => $a < $b,
    '<=' => fn($a, $b) => $a <= $b,
    '>' => fn($a, $b) => $a > $b,
    '>=' => fn($a, $b) => $a >= $b,
];
echo "@binop\n";
// a  b  op  diags  result
foreach ($B as $i => $a) {
    foreach ($B as $j => $b) {
        foreach ($ops as $name => $f) {
            // intdiv() is a function with int parameters: only ints are interesting.
            if ($name === 'intdiv' && !(is_int($a) && is_int($b))) continue;
            echo "$i\t$j\t$name\t", run(fn() => $f($a, $b)), "\n";
        }
    }
}

// String/string comparisons (numeric vs lexical).
$S = [
    "abc", "abd", "ab", "", "10", "9", "1e1", "010", "0.0", "-0", " 1", "1 ",
    "1", "1.0", "INF", "0x10", "16", "1e1000", "9223372036854775807",
    "9223372036854775808", "abc\x00", "Abc", " ", "00",
];
echo "@svalues\n";
foreach ($S as $i => $v) echo "$i\t", enc($v), "\n";
echo "@scmp\n";
foreach ($S as $i => $a) {
    foreach ($S as $j => $b) {
        echo "$i\t$j\t", ($a == $b) ? 1 : 0, "\t", $a <=> $b, "\n";
    }
}

// ---------------------------------------------------------------- floats
// <bits> <(string) at precision 14, raw> <precision 17> <precision -1> <precision 1> <precision 5> <precision 20>
mt_srand(12345);
$F = [
    0.0, -0.0, 1.0, -1.0, 0.1, 0.2, 0.3, 0.1 + 0.2, 1 / 3, 2 / 3, 10 / 3,
    0.5, 1.5, 2.5, 3.5, 0.125, 0.375, 1e-4, 1e-5, 0.0001234, 0.00001234,
    123456789012345.0, 1234567890123456.0, 12345678901234567.0, 1e14, 1e15,
    1e16, 1e17, 1e20, 1e21, 1e22, 1e23, 1e100, 1e-100, 1e300, 1e308,
    1.7976931348623157e308, 2.2250738585072014e-308, 2.225073858507201e-308,
    4.9e-324, 9.8813129168249309e-324, 5e-324 * 3, 123.456, -123.456,
    99999999999999.0, 999999999999999.0, 9999999999999999.0, 0.999999999999999,
    0.9999999999999999, 99999.99999999999, 1.0E+25, 9.2233720368547758E18,
    PHP_INT_MAX + 1.0, 4503599627370496.0, 9007199254740992.0,
    9007199254740993.0, 1.5e-7, 2.0e-7, 7e22, 5e-1, 0.05, 0.005, 1.05, 1.005,
    1.0000000000000002, 0.30000000000000004, 1234.5, 12345.5, 0.95, 0.095,
    INF, -INF, NAN, 100.0, 1000000.0, 123e18, 3.14159265358979, M_PI, M_E,
];
for ($k = -30; $k <= 30; $k++) $F[] = 10.0 ** $k;
for ($k = -30; $k <= 30; $k++) $F[] = 1.2345678901234567 * 10.0 ** $k;
for ($i = 0; $i < 600; $i++) {
    // Random bit patterns (finite only).
    do {
        $bits = '';
        for ($b = 0; $b < 8; $b++) $bits .= chr(mt_rand(0, 255));
        $f = unpack('E', $bits)[1];
    } while (is_nan($f) || is_infinite($f));
    $F[] = $f;
}
for ($i = 0; $i < 400; $i++) {
    // "Nice" decimals of various magnitudes.
    $F[] = round(mt_rand() / mt_getrandmax() * 10 ** mt_rand(-8, 20), mt_rand(0, 10));
}
for ($i = 0; $i < 200; $i++) {
    // Short decimal inputs with ties and near-ties.
    $F[] = (float)(mt_rand(1, 99999) . "." . mt_rand(0, 99999) . "5");
}
echo "@floats\n";
foreach ($F as $f) {
    $row = [bin2hex(pack('E', $f))];
    foreach ([14, 17, -1, 1, 5, 20] as $p) {
        ini_set('precision', $p);
        $row[] = (string)$f;
    }
    ini_set('precision', 14);
    echo implode("\t", $row), "\n";
}

// ---------------------------------------------------------------- strings to float
// Decimal strings parsed as numbers: <hex string> <result bits>
$P = ["0.1", "1e23", "8.98846567431158e307", "2.2250738585072011e-308",
    "2.2250738585072012e-308", "4.9406564584124654e-324", "2.4703282292062327e-324",
    "2.4703282292062328e-324", "1.7976931348623158e308", "1.7976931348623159e308",
    "123456789012345678901234567890", "0.000000000000000000000000000001",
    "9007199254740993", "9007199254740992.5", "1.00000000000000011102230246251565404236316680908203125",
    "1." . str_repeat("0", 400) . "1", str_repeat("9", 400), "." . str_repeat("0", 330) . "1"];
for ($i = 0; $i < 100; $i++) {
    $P[] = mt_rand(0, 999999999) . "." . mt_rand(0, 999999999) . "e" . mt_rand(-320, 300);
}
echo "@parse\n";
foreach ($P as $s) echo bin2hex($s), "\t", run(fn() => $s + 0), "\n";

// ---------------------------------------------------------------- inc/dec
$I = ["a", "z", "A", "Z", "0", "9", "Az", "aZ", "A9", "a9", "Zz", "zZ", "9z",
    "9Z", "zz", "ZZ", "99", "a!", "!a", "-cc", "Z ", " Z", "foo1.txt", "1f.5",
    "5d9", "5e9", "1e", "199Z", "Hello world", "\xff", "", " ", "abc", "a9z",
    "9", "-9", "1.5", "-1.5", "9223372036854775807", "-9223372036854775808",
    " 1", "1 ", "0x1", "zZ9", "Zz9", "e", "1e1", "1E1"];
echo "@incdec\n";
foreach ($I as $s) {
    echo bin2hex($s), "\tinc\t", run(function () use ($s) { $x = $s; $x++; return $x; }), "\n";
    echo bin2hex($s), "\tdec\t", run(function () use ($s) { $x = $s; $x--; return $x; }), "\n";
}
echo "@end\n";
