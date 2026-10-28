<?php
// Driver for math.txt: one line per case: name TAB inputs TAB result.
// Floats are written as their bit pattern (big-endian hex) after "f:".
function enc($v) {
    if (is_int($v)) return "i:$v";
    if (is_float($v)) return "f:" . bin2hex(pack('E', $v));
    if (is_bool($v)) return "b:" . (int)$v;
    if (is_null($v)) return "n:";
    if ($v instanceof UnitEnum) return "m:" . $v->name;
    if (is_array($v)) return "a:" . bin2hex(json_encode($v));
    return "s:" . bin2hex($v);
}
function run($name, $fn, ...$args) {
    $ws = [];
    set_error_handler(function ($no, $str) use (&$ws) { $ws[] = $str; return true; });
    try { $r = "R:" . enc($fn(...$args)); }
    catch (Throwable $t) { $r = "E:" . get_class($t) . ":" . $t->getMessage(); }
    restore_error_handler();
    foreach ($ws as $w) $r .= "\tW:$w";
    echo $name, "\t", implode(',', array_map('enc', $args)), "\t", $r, "\n";
}
$vals = [0.285, 1.955, 5.045, 5.055, 0.49999999999999994, 0.50000000000000011, 0.5, 1.5, 2.5, -2.5, 3.5, 1.45, 1.55, 2.675, 1.005, 0.1 + 0.2, 1234.5678, -1234.5678,
  12345678901234565.0, 4503599627370495.5, 0.12345678901234565, 1.700000000000145, 123456789012344.5, 1.500000000000001, 1e15 + 0.5, 1e16, 1e22, 1e-22, 2e-23, 5e-324, 1.7976931348623157e308,
  0.0, -0.0, INF, -INF, NAN, 15.5, 25.5, -0.0001, 0.0001, 7.0, 15.51, 0.61, 0.65, -0.65, 1.9999, 999.5, 9.995, 99.995, 0.045, 0.055, 1.0000000000000002, 0.30000000000000004, 1e-300, 3.0e-5, 1.15, 1.25, 1.35, 8.325, 8.335, 1.4999999999999998, 2.5000000000000004,
  123456.789e10, 0.1234567890123456789, 1.2345678901234567, 12.345678901234567, 1e14 + 0.5, 1e13 + 0.25];
$places = [-30, -5, -3, -2, -1, 0, 1, 2, 3, 5, 10, 14, 15, 16, 17, 20, 22, 25, 300, 330];
foreach ([PHP_ROUND_HALF_UP, PHP_ROUND_HALF_DOWN, PHP_ROUND_HALF_EVEN, PHP_ROUND_HALF_ODD] as $m)
  foreach ($vals as $v) foreach ($places as $p) run("round", 'round', $v, $p, $m);
foreach ([RoundingMode::TowardsZero, RoundingMode::AwayFromZero, RoundingMode::NegativeInfinity, RoundingMode::PositiveInfinity] as $m)
  foreach ($vals as $v) foreach ([-2, -1, 0, 1, 2, 3, 16] as $p) run("round_" . $m->name, 'round', $v, $p, $m);
foreach ([0, 1, -1, 15, 25, -25, 1234567, PHP_INT_MAX, PHP_INT_MIN, 9007199254740993, 4611686018427387903] as $v) foreach ([-20, -19, -18, -5, -1, 0, 1, 2] as $p) run("round", 'round', $v, $p);
run("round", 'round', 1.0, -2147483648);
run("round", 'round', 1.0, PHP_INT_MAX);
run("round", 'round', 1.0, 2, 99);
run("ceil", 'ceil', 1.5); run("ceil", 'ceil', -1.5); run("ceil", 'ceil', -0.5); run("ceil", 'ceil', 5); run("floor", 'floor', -0.5); run("floor", 'floor', 0.5); run("floor", 'floor', -5);
foreach ([[5.7, 1.3], [-5.7, 1.3], [5.7, -1.3], [1, 0], [0, 0], [INF, 1], [1, INF], [-0.0, 1], [1e300, 3.0], [10, 3]] as [$a, $b]) run("fmod", 'fmod', $a, $b);
foreach ([[7, 2], [-7, 2], [7, -2], [1, 0], [PHP_INT_MIN, -1], [PHP_INT_MIN, 1], [0, 5]] as [$a, $b]) run("intdiv", 'intdiv', $a, $b);
// number_format
$nf = [0.0, -0.0, 0.5, 1.5, 2.5, -0.4, -0.5, 1234.5678, -1234.5678, 1234567.891, 0.285, 1.005, 1e15, 1e20, 1.7976931348623157e308, 1e-10, -1e-10, INF, -INF, NAN, 999.5, 0.125, 123456789012345678.0, 15151, -15151, PHP_INT_MAX, PHP_INT_MIN, 999, 0, 5, 12345, -999999];
foreach ($nf as $v) foreach ([0, 1, 2, 3, 5, -1, -3, -19, -20, 20, 60, PHP_INT_MIN] as $d) run("number_format", 'number_format', $v, $d);
foreach ([[1234567.891, 2, ',', '.'], [1234567.891, 2, '', ''], [1234567.891, 2, 'DEC', 'THOUSANDS'], [-1234567.891, 3, '.', ' '], [1234.5, 0, '.', ','], [1234.5, 2, '', ','], [0.5, 2, '', ''], [-0.5, 0, '.', ','], [1e6, 0, '.', '..']] as $a) run("number_format", 'number_format', ...$a);
run("number_format", 'number_format', 1234.5678);
run("number_format", 'number_format', 1234.5678, 2, null, null);
// base conversions
$bs = ['FF', 'ff', '0xFf', ' 0xFF\t', '0b101', '0o17', '17', '-17', '1.5', '', 'zz', 'ZZ', 'fg', 'f 0xff ', '1xff ', "\0", '0x', '0b', '0', '00', "\n\r 12 \v\f", '7fffffffffffffff', '8000000000000000', 'ffffffffffffffff', '10000000000000000', 'ffffffffffffffffffffffff', '1e5', '+5'];
foreach ($bs as $s) { run("hexdec", 'hexdec', $s); run("octdec", 'octdec', $s); run("bindec", 'bindec', $s); }
foreach (['111111111111111111111111111111111111111111111111111111111111111', '1000000000000000000000000000000000000000000000000000000000000000', '1111111111111111111111111111111111111111111111111111111111111111', '11111111111111111111111111111111111111111111111111111111111111111111'] as $s) run("bindec", 'bindec', $s);
foreach (['777777777777777777777', '1000000000000000000000', '1777777777777777777777', '7777777777777777777777777'] as $s) run("octdec", 'octdec', $s);
foreach ([['FF', 16, 10], ['255', 10, 16], ['255', 10, 2], ['255', 10, 36], ['zz', 36, 10], ['ZZ', 36, 2], ['-255', 10, 16], ['ffffffffffffffff', 16, 10], ['ffffffffffffffffffff', 16, 10], ['ffffffffffffffffffff', 16, 16], ['18446744073709551616', 10, 16], ['99999999999999999999999', 10, 36], ['0', 10, 2], ['', 10, 2], ['1.5', 10, 2], ['FF', 1, 10], ['FF', 16, 37], ['FF', 37, 10], ['0o7', 9, 10], ['0 0', 9, 10], ['0b1010', 2, 10], ['0B1010', 2, 10], ['0X1f', 16, 2], ['9223372036854775807', 10, 16], ['9223372036854775808', 10, 16], ['1e5', 10, 10]] as $a) run("base_convert", 'base_convert', ...$a);
foreach ([0, 1, -1, 255, PHP_INT_MAX, PHP_INT_MIN, 1234567] as $v) { run("decbin", 'decbin', $v); run("dechex", 'dechex', $v); run("decoct", 'decoct', $v); }
// sprintf ordering checks
foreach ([['%y %s'], ['%s%5%', 'a'], ['%y'], ['%'], ['%5%%s', 'a']] as $a) run("sprintf", 'sprintf', ...$a);
run("vsprintf", 'vsprintf', '%s %s', ['a']);
run("vsprintf", 'vsprintf', '%2$s', ['a']);
run("vsprintf", 'vsprintf', '%s', []);
run("vsprintf", 'vsprintf', '%*s', ['a']);
run("vsprintf", 'vsprintf', '%5%', []);
run("mt_getrandmax", 'mt_getrandmax');
run("getrandmax", 'getrandmax');
