<?php
// Generates tests/fixtures/gmp_php85.txt: run once on the reference PHP 8.5
// (with ext/gmp, GMP 6.3) as
//   php -d error_reporting=-1 -d display_errors=1 gmp_driver.php > gmp_php85.txt
// Every line is one case: the operation, its encoded arguments, "=>", the
// diagnostics raised (D:/W: messages) and the encoded outcome, separated
// by tabs. tests/php85_fixture.rs replays them against goro-ext-gmp.
//
// Argument encoding: i<int>, q<printable ASCII string>, s<hex bytes>, g<decimal> (a GMP object),
// f<hex of the IEEE-754 bits>, b0/b1, n (null), a (array), o (stdClass),
// t (object of class T with __toString), r (resource).
// Outcomes: G<decimal> (GMP), I<int>, B0/B1, S<hex>, F (false), N (null),
// D<hex float bits>, A(x;y;...) (array of outcomes), E<class>:<message>;
// long decimal/hex outcomes are written H<length>:<crc32> instead.

require __DIR__ . '/gmp_driver_strict.php';

class T { public function __toString(): string { return "42"; } }

mt_srand(20261028);
$RES = fopen('php://memory', 'r');

$notes = [];
$DEFS = [];
set_error_handler(function ($no, $str) use (&$notes) {
    $notes[] = ($no === E_DEPRECATED ? 'D:' : 'W:') . $str;
    return true;
});

function enc_arg($v) {
    global $RES, $DEFS;
    if ($v instanceof GMP) {
        $d = gmp_strval($v);
        if (strlen($d) <= 40) return 'g' . $d;
        // Long values are defined once ("#def <index> <decimal>") and then
        // referred to as p<index>.
        if (!isset($DEFS[$d])) {
            $DEFS[$d] = count($DEFS);
            echo "#def\t", $DEFS[$d], "\t", $d, "\n";
        }
        return 'p' . $DEFS[$d];
    }
    if ($v instanceof T) return 't';
    if (is_object($v)) return 'o';
    if (is_int($v)) return 'i' . $v;
    if (is_string($v)) return preg_match('/^[\x20-\x7e]*\z/', $v) ? 'q' . $v : 's' . bin2hex($v);
    if (is_float($v)) return 'f' . bin2hex(strrev(pack('d', $v)));
    if (is_bool($v)) return $v ? 'b1' : 'b0';
    if ($v === null) return 'n';
    if (is_array($v)) return 'a';
    if (is_resource($v)) return 'r';
    throw new Exception('cannot encode');
}

function long_or($prefix, $s) {
    if (strlen($s) > 48) return 'H' . strlen($s) . ':' . sprintf('%08x', crc32($s));
    return $prefix . $s;
}

function enc_out($v) {
    if ($v instanceof GMP) return long_or('G', gmp_strval($v));
    if (is_int($v)) return 'I' . $v;
    if (is_bool($v)) return $v ? 'B1' : 'B0';
    if (is_string($v)) return long_or('S', bin2hex($v));
    if (is_float($v)) return 'D' . bin2hex(strrev(pack('d', $v)));
    if ($v === null) return 'N';
    if (is_array($v)) return 'A(' . implode(';', array_map('enc_out', $v)) . ')';
    return '?';
}

function run($name, array $args, callable $f) {
    global $notes;
    $notes = [];
    try {
        $out = enc_out($f(...$args));
    } catch (Throwable $e) {
        $out = 'E' . get_class($e) . ':' . $e->getMessage();
    }
    $line = $name;
    foreach ($args as $a) $line .= "\t" . enc_arg($a);
    $line .= "\t=>";
    foreach ($notes as $n) $line .= "\t" . $n;
    echo $line, "\t", $out, "\n";
}

// A gmp function call, in coercive mode.
function call($fn, ...$args) { run($fn, $args, $fn); }
// The same call from a strict_types=1 file.
function call_strict($fn, ...$args) { run('!' . $fn, $args, fn(...$a) => strict_call($fn, $a)); }

function op($op, $a, $b) {
    $f = match ($op) {
        '+' => fn($a, $b) => $a + $b,
        '-' => fn($a, $b) => $a - $b,
        '*' => fn($a, $b) => $a * $b,
        '/' => fn($a, $b) => $a / $b,
        '%' => fn($a, $b) => $a % $b,
        '**' => fn($a, $b) => $a ** $b,
        '&' => fn($a, $b) => $a & $b,
        '|' => fn($a, $b) => $a | $b,
        '^' => fn($a, $b) => $a ^ $b,
        '<<' => fn($a, $b) => $a << $b,
        '>>' => fn($a, $b) => $a >> $b,
        '<=>' => fn($a, $b) => $a <=> $b,
        '==' => fn($a, $b) => $a == $b,
        '!=' => fn($a, $b) => $a != $b,
        '<' => fn($a, $b) => $a < $b,
        '<=' => fn($a, $b) => $a <= $b,
        '>' => fn($a, $b) => $a > $b,
        '>=' => fn($a, $b) => $a >= $b,
    };
    run('op' . $op, [$a, $b], $f);
}

function unary($op, $a) {
    $f = match ($op) {
        '-' => fn($a) => -$a,
        '~' => fn($a) => ~$a,
        '+' => fn($a) => +$a,
        '++' => function ($a) { $a++; return $a; },
        '--' => function ($a) { $a--; return $a; },
        'int' => fn($a) => (int) $a,
        'float' => fn($a) => (float) $a,
        'bool' => fn($a) => (bool) $a,
        'string' => fn($a) => (string) $a,
        'serialize' => fn($a) => serialize($a),
        '__serialize' => fn($a) => $a->__serialize(),
    };
    run('u' . $op, [$a], $f);
}

function g($v) { return gmp_init($v); }

function rand_digits($n) {
    $s = (string) mt_rand(1, 9);
    for ($i = 1; $i < $n; $i++) $s .= mt_rand(0, 9);
    return $s;
}

function rand_bits_value($bits) {
    $v = gmp_init(0);
    for ($i = 0; $i < $bits; $i += 30) {
        $v = gmp_add(gmp_mul($v, 1 << 30), mt_rand(0, (1 << 30) - 1));
    }
    $v = gmp_div_q($v, gmp_pow(2, max(0, (int) (ceil($bits / 30) * 30) - $bits)));
    gmp_setbit($v, $bits - 1);
    return $v;
}

// ------------------------------------------------------------ operand pools

$small = [0, 1, -1, 2, -2, 3, -3, 5, 7, -7, 10, 16, 17, -42, 100, 255, -256, 1000, 65536];
$edge_ints = [2147483647, 2147483648, -2147483648, 4294967296, 9007199254740993,
    PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX - 1, -PHP_INT_MAX];
$big = [
    '9223372036854775808', '-9223372036854775809', '18446744073709551615', '18446744073709551616',
    '-18446744073709551616', '18446744073709551617', '36893488147419103237', '-36893488147419103237',
    '100000000000000000000', '-100000000000000000000', '170141183460469231731687303715884105727',
    '340282366920938463463374607431768211456', gmp_strval(gmp_pow(3, 50)), gmp_strval(gmp_neg(gmp_pow(7, 40))),
];
foreach ([70, 100, 128, 160, 200, 256, 300, 512, 1000] as $b) {
    $v = rand_bits_value($b);
    $big[] = gmp_strval($v);
    if ($b % 200 == 0 || $b == 100 || $b == 256) $big[] = gmp_strval(gmp_neg($v));
}
$pool = array_merge($small, $edge_ints, $big);
$gpool = array_map('g', $pool);
// a smaller pool for the pairwise operations
$core_vals = [0, 1, -1, 2, -3, 7, -42, 255, 65536, PHP_INT_MAX, PHP_INT_MIN,
    '9223372036854775808', '18446744073709551616', '-18446744073709551617', '36893488147419103237',
    '-100000000000000000000', $big[16], $big[17], $big[20], $big[22], $big[25]];
$core = array_map('g', $core_vals);

// ---------------------------------------------------------------- parsing

$ws = ['', ' ', '  ', "\t", "\n", "\x0b", "\x0c", "\r"];
$signs = ['', '-', '+', '--', '- '];
$prefixes = ['', '0x', '0X', '0b', '0B', '0o', '0O', '0', '00', '0 x', '0x ', '0x-'];
$bodies = ['', '0', '1', '7', '8', '9', 'a', 'f', 'g', 'z', 'A', 'F', 'Z', '12', '1 2', '1_2', '10',
    'ff', 'FF', 'fF', '-1', ' 1', '1 ', "1\0", "1\x002", "\x001", '1.5', '1e3', '0x1', 'zz', 'ZZ', 'Zz', 'zZ',
    '11', '101', '777', '1' . str_repeat('0', 30), str_repeat('f', 40), str_repeat('z', 30), str_repeat('Z', 30),
    "1\xa02", "\xff", '١'];
$suffixes = ['', ' ', "\n", 'x', '-', "\0z"];
$parse_strings = [];
foreach ($ws as $w) foreach ($signs as $sg) foreach ($prefixes as $p) foreach ($bodies as $b) {
    if (mt_rand(0, 19) < 2 || ($w === '' && $sg === '')) $parse_strings[] = $w . $sg . $p . $b;
}
foreach ($bodies as $b) foreach ($suffixes as $sf) $parse_strings[] = $b . $sf;
foreach (['0x', '0b', '0o', ''] as $p) foreach ($ws as $w) $parse_strings[] = $w . $p . $w;
$parse_strings = array_values(array_unique($parse_strings));
$bases = [0, 2, 8, 10, 16, 36, 37, 62];
foreach ($parse_strings as $str) {
    foreach ($bases as $base) {
        if ($base === 0 || $base === 16 || mt_rand(0, 9) < 4) call('gmp_init', $str, $base);
    }
}
// every base on a few strings
foreach (['10', 'z', 'Z', 'zz', 'ZZ', '0x10', '0b10', '0o10', '-10', '9', ' 1 0 ', '-0x', '-0b'] as $str) {
    for ($base = -2; $base <= 64; $base++) call('gmp_init', $str, $base);
}
// random long digit strings in many bases
$digits62 = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
foreach ([2, 3, 7, 8, 10, 16, 31, 36, 37, 50, 62] as $base) {
    foreach ([1, 5, 15, 19, 20, 21, 40, 64, 65, 120, 300] as $len) {
        $s = '';
        for ($i = 0; $i < $len; $i++) $s .= $digits62[mt_rand(0, $base - 1)];
        if ($base <= 36 && mt_rand(0, 1)) $s = strtolower($s);
        call('gmp_init', $s, $base);
        call('gmp_init', '-' . $s, $base);
    }
}
// gmp_init argument types
foreach ([12, -12, 12.0, 12.5, -0.5, 1e30, -1e19, NAN, INF, true, false, null, [], new stdClass, new T, $RES, g(77), '77'] as $v) {
    call('gmp_init', $v);
    call('gmp_init', $v, 16);
    call('gmp_init', $v, 1);
    call_strict('gmp_init', $v);
}
call('gmp_init', '', 1);
call('gmp_init', 'x', 63);

// ----------------------------------------------------- argument conversion

call('gmp_random_seed', 1);

$weird = [12.0, 12.5, -3.75, 0.1, 1e-5, 1e15 + 0.5, 1e30, -1e30, 9.2233720368547758E+18, -9.2233720368547758E+18,
    NAN, INF, -INF, -0.0, true, false, null, [], new stdClass, new T, $RES, ' 12', '12 ', '0x1A', '-0b101', '012',
    '0o17', '+5', '1.0', '', ' ', 'abc'];
$one_arg = ['gmp_abs', 'gmp_neg', 'gmp_intval', 'gmp_strval', 'gmp_com', 'gmp_sqrt', 'gmp_sign', 'gmp_popcount',
    'gmp_nextprime', 'gmp_perfect_square', 'gmp_perfect_power', 'gmp_prob_prime', 'gmp_export',
    'gmp_sqrtrem'];
foreach ($weird as $v) {
    foreach ($one_arg as $fn) call($fn, $v);
    call_strict('gmp_abs', $v);
    call('gmp_add', $v, 1);
    call('gmp_add', 1, $v);
    call('gmp_add', $v, $v);
    call_strict('gmp_add', 1, $v);
    call('gmp_powm', $v, 2, 7);
    call('gmp_powm', 3, $v, 7);
    call('gmp_powm', 3, 2, $v);
    call('gmp_random_range', $v, 100);
    call('gmp_random_range', -100, $v);
    call('gmp_binomial', $v, 2);
    call('gmp_testbit', $v, 1);
    call('gmp_scan0', $v, 1);
    call('gmp_root', $v, 2);
}
foreach ([12.5, true, false, null, [], 'abc', '', -3, '-3', 1e30, NAN] as $v) call('gmp_fact', $v);
// error order: conversion vs. value checks
call('gmp_pow', 'x', -1);
call('gmp_pow', -2, -1);
call('gmp_root', 'x', 0);
call('gmp_root', -4, 0);
call('gmp_root', -4, 2);
call('gmp_root', -4, -2);
call('gmp_rootrem', -4, 0);
call('gmp_rootrem', -4, 2);
call('gmp_rootrem', 'x', 0);
call('gmp_testbit', 'x', -1);
call('gmp_scan1', 'x', -1);
call('gmp_binomial', 'x', -1);
call('gmp_powm', 3, -1, 'x');
call('gmp_powm', 3, -1, 0);
call('gmp_powm', 'x', -1, 0);
call('gmp_powm', 3, 'x', 0);
call('gmp_div_q', 1, 0, 9);
call('gmp_div_q', 'x', 0, 9);
call('gmp_div_r', 1, 'x', 9);
call('gmp_div_qr', 1, 2, 9);
call('gmp_div', 1, 0, 0);
call('gmp_div', 1, 2, 3);
call('gmp_mod', 'x', 0);
call('gmp_mod', 1, 0);
call('gmp_divexact', 1, 0);
call('gmp_invert', 1, 0);
call('gmp_invert', 'x', 0);
call('gmp_strval', 'x', 1);
call('gmp_strval', 5, 1);
call('gmp_export', 'x', 0);
call('gmp_export', 5, 0, -1);
call('gmp_export', 5, 1, -1);
call('gmp_export', 5, 1, GMP_LITTLE_ENDIAN | GMP_NATIVE_ENDIAN);
call('gmp_import', 'abc', 2, -1);
call('gmp_import', 'abc', 0, -1);
call('gmp_import', 'abcd', 2, -1);
call('gmp_import', 'abcd', 2, GMP_BIG_ENDIAN | GMP_NATIVE_ENDIAN);
call('gmp_import', 'abc', 2, GMP_BIG_ENDIAN | GMP_NATIVE_ENDIAN);
call('gmp_random_range', 5, 5);
call('gmp_random_range', 6, 5);
call('gmp_random_range', 'x', 5);
call('gmp_random_bits', 0);
call('gmp_random_bits', -1);
call('gmp_random_bits', 2147483648);
call('gmp_random_bits', PHP_INT_MAX);
call('gmp_random_bits', PHP_INT_MIN);
foreach ([-1, 0, 1, 137438953406, 137438953407, 137438953408, 137438953409, PHP_INT_MAX] as $i) {
    call('gmp_testbit', 5, $i);
    call('gmp_testbit', -5, $i);
    call('gmp_scan0', 5, $i);
    call('gmp_scan1', 5, $i);
    call('gmp_scan0', -5, $i);
    call('gmp_scan1', -5, $i);
    if ($i < 1000) {
        run('setbit', [g(5), $i, true], function ($g, $i, $v) { gmp_setbit($g, $i, $v); return $g; });
        run('clrbit', [g(5), $i], function ($g, $i) { gmp_clrbit($g, $i); return $g; });
    } else {
        run('setbit', [g(-5), $i, true], function ($g, $i, $v) { gmp_setbit($g, $i, $v); return $g; });
        run('clrbit', [g(5), $i], function ($g, $i) { gmp_clrbit($g, $i); return $g; });
    }
}
foreach (['5', 5, 5.0, null, [], new stdClass] as $v) {
    run('setbit', [$v, 1, true], function ($g, $i, $v) { gmp_setbit($g, $i, $v); return $g; });
    run('clrbit', [$v, 1], function ($g, $i) { gmp_clrbit($g, $i); return $g; });
}
foreach ([1, 2, 7, 8, 100] as $ws_) {
    foreach (['0', '1', '255', '-65536', $big[16], $big[25]] as $v) {
        foreach ([0, 1, 2, 4, 8, 16, 1 | 4, 1 | 8, 2 | 4, 2 | 8, 1 | 16, 2 | 16, 1024, -1, 3, 12, 20, 24, 28, PHP_INT_MIN] as $fl) {
            call('gmp_export', $v, $ws_, $fl);
        }
    }
}
foreach ([1 << 20, 1 << 40, 1 << 59, (1 << 61) - 8, (1 << 61) - 1, 1 << 61, (1 << 62) - 1, 1 << 62, PHP_INT_MAX - 1000, PHP_INT_MAX] as $ws_) {
    foreach (['0', '1', '255', '-65536', '9223372036854775808', $big[16], $big[25]] as $v) {
        // Observed on PHP 8.5: the word size is refused when 8·size + bits - 1
        // overflows 64 bits; below that PHP allocates the result, so only
        // small ones are run.
        $over = gmp_cmp(gmp_add(gmp_mul(8, $ws_), max(1, strlen(gmp_strval(gmp_abs($v), 2))) - 1), gmp_pow(2, 64)) >= 0;
        if ($v === '0' || $over) call('gmp_export', $v, $ws_);
        elseif ($ws_ <= (1 << 20)) run('gmp_export_len', [$v, $ws_], fn($v, $w) => strlen(gmp_export($v, $w)));
    }
}
foreach (['', "\x01", "\x01\x02", "\x00\x00\x01\x02", "\x12\x34\x56\x78\x9a\xbc", "\xff\xff\xff\xff\xff\xff\xff\xff\xff",
    str_repeat("\x80\x01\x02", 12), str_repeat("\x00", 16)] as $data) {
    foreach ([1, 2, 3, 4, 6, 8, 9, 12, 16] as $ws_) {
        foreach ([0, 1 | 4, 1 | 8, 2 | 4, 2 | 8, 1 | 16, 2 | 16, 16, 3, 12, 20, 1 << 20] as $fl) {
            call('gmp_import', $data, $ws_, $fl);
        }
    }
}

// ------------------------------------------------------------- functions

foreach ($gpool as $a) {
    foreach ([2, 3, 8, 10, 16, 36, 37, 62, -2, -16, -36] as $b) call('gmp_strval', $a, $b);
    call('gmp_intval', $a);
    call('gmp_neg', $a);
    call('gmp_abs', $a);
    call('gmp_com', $a);
    call('gmp_sqrt', $a);
    call('gmp_sqrtrem', $a);
    call('gmp_sign', $a);
    call('gmp_popcount', $a);
    call('gmp_perfect_square', $a);
    call('gmp_perfect_power', $a);
    call('gmp_prob_prime', $a);
    call('gmp_nextprime', $a);
    if (gmp_cmp(gmp_abs($a), 2000) < 0) call('gmp_fact', $a);
    foreach ([0, 1, 2, 3, 5, 10, 63, 64, 100] as $e) {
        if (gmp_cmp(gmp_abs($a), 1 << 20) > 0 && $e > 10) continue;
        call('gmp_pow', $a, $e);
    }
    foreach ([1, 2, 3, 4, 5, 7, 64, 1000, PHP_INT_MAX] as $n) {
        call('gmp_root', $a, $n);
        call('gmp_rootrem', $a, $n);
    }
    foreach ([0, 1, 2, 5, 13, 64] as $k) call('gmp_binomial', $a, $k);
    foreach ([0, 1, 2, 3, 31, 63, 64, 65, 100, 200, 1000] as $i) {
        call('gmp_testbit', $a, $i);
        call('gmp_scan0', $a, $i);
        call('gmp_scan1', $a, $i);
    }
    foreach ([0, 1, 7, 64, 99] as $i) {
        foreach ([true, false] as $bv) {
            run('setbit', [$a, $i, $bv], function ($g, $i, $v) { $g = clone $g; gmp_setbit($g, $i, $v); return $g; });
        }
        run('clrbit', [$a, $i], function ($g, $i) { $g = clone $g; gmp_clrbit($g, $i); return $g; });
    }
    foreach ([1, 2, 3, 4, 8, 9, 16] as $ws_) {
        call('gmp_export', $a, $ws_, GMP_MSW_FIRST | GMP_BIG_ENDIAN);
        call('gmp_export', $a, $ws_, GMP_LSW_FIRST | GMP_LITTLE_ENDIAN);
        call('gmp_export', $a, $ws_, GMP_MSW_FIRST | GMP_LITTLE_ENDIAN);
    }
    foreach (['-', '~', '+', '++', '--', 'int', 'float', 'bool', 'string', 'serialize', '__serialize'] as $u) unary($u, $a);
}

$binfns = ['gmp_add', 'gmp_sub', 'gmp_mul', 'gmp_mod', 'gmp_divexact', 'gmp_gcd', 'gmp_gcdext', 'gmp_lcm',
    'gmp_invert', 'gmp_jacobi', 'gmp_legendre', 'gmp_kronecker', 'gmp_cmp', 'gmp_and', 'gmp_or', 'gmp_xor',
    'gmp_hamdist', 'gmp_random_range_check'];
foreach ($core as $a) {
    foreach ($core as $b) {
        foreach ($binfns as $fn) {
            if ($fn === 'gmp_random_range_check') {
                if (gmp_cmp($a, $b) >= 0) call('gmp_random_range', $a, $b);
                continue;
            }
            call($fn, $a, $b);
        }
        foreach ([GMP_ROUND_ZERO, GMP_ROUND_PLUSINF, GMP_ROUND_MINUSINF] as $m) {
            call('gmp_div_qr', $a, $b, $m);
            call('gmp_div_q', $a, $b, $m);
            call('gmp_div_r', $a, $b, $m);
        }
        foreach (['+', '-', '*', '/', '%', '&', '|', '^', '<=>', '==', '<', '>='] as $o) op($o, $a, $b);
    }
}
// divexact on inexact operands (GMP's 2-adic result)
foreach ([[7, 3], [-7, 3], [100, 7], [100, 6], [101, 12], ['100000000000000000000000000001', 3], [5, 10], [5, -10],
    ['18446744073709551618', 6], ['18446744073709551617', 4], ['36893488147419103233', 12], ['340282366920938463463374607431768211457', 10],
    ['340282366920938463463374607431768211457', '18446744073709551618'], ['340282366920938463463374607431768211457', '36893488147419103236'],
    [3, '18446744073709551616'], ['18446744073709551617', '18446744073709551616'], ['36893488147419103233', '18446744073709551616'],
    [1, 2], [-1, 2], [12345, 64], ['1' . str_repeat('0', 60), 7], ['1' . str_repeat('0', 60), '1' . str_repeat('0', 25) . '3']] as [$x, $y]) {
    call('gmp_divexact', $x, $y);
}
// powm
$mods = [1, -1, 2, 7, -7, 10, 1000, 65537, '18446744073709551557', '18446744073709551616', $big[17], $big[20], $big[25]];
foreach ([0, 1, -1, 2, 3, -5, 77, $big[16], $big[18], $big[25]] as $x) {
    foreach ([0, 1, 2, 3, 17, 65537, $big[16], $big[20]] as $e) {
        foreach ($mods as $m) call('gmp_powm', g($x), g($e), g($m));
    }
}
// primes
$primes = [2, 3, 5, 7, 11, 13, 1531, 999983, 1000003, '1000000000039', '2462906046218231', '2462906046218251',
    '2251799813685269', '4503599627370517', '9223372036854775837', '18446744073709551557', '18446744073709551629',
    '170141183460469231731687303715884105727', '2305843009213693951', '618970019642690137449562111'];
$composites = [0, 1, 4, 9, 15, 561, 1105, 25326001, '3215031751', '2152302898747', '3474749660383', '341550071728321',
    '3825123056546413051', '318665857834031151167461', '2462906046218233', '18446744073709551617', '340282366920938463463374607431768211457'];
foreach (array_merge($primes, $composites) as $n) {
    foreach ([0, 1, 10, 25, 30, -1] as $r) {
        call('gmp_prob_prime', $n, $r);
        call('gmp_prob_prime', gmp_neg($n), $r);
    }
    call('gmp_nextprime', $n);
    call('gmp_nextprime', gmp_neg($n));
}
$p = gmp_init('2462906046218000');
for ($i = 0; $i < 30; $i++) { $p = gmp_nextprime($p); call('gmp_prob_prime', $p); }
for ($i = 0; $i < 300; $i++) {
    $n = gmp_random_range(2, gmp_pow(2, mt_rand(2, 60)));
    call('gmp_prob_prime', $n);
    call('gmp_nextprime', $n);
}
// number theory over random small pairs
for ($i = 0; $i < 600; $i++) {
    $x = mt_rand(-1000, 1000);
    $y = mt_rand(-1000, 1000);
    foreach (['gmp_gcdext', 'gmp_invert', 'gmp_jacobi', 'gmp_legendre', 'gmp_kronecker', 'gmp_lcm'] as $fn) call($fn, $x, $y);
}
for ($i = 0; $i < 200; $i++) {
    $x = gmp_random_range(gmp_neg(gmp_pow(2, 200)), gmp_pow(2, 200));
    $y = gmp_random_range(gmp_neg(gmp_pow(2, 150)), gmp_pow(2, 150));
    foreach (['gmp_gcdext', 'gmp_invert', 'gmp_jacobi', 'gmp_kronecker'] as $fn) call($fn, $x, $y);
}
foreach ([[0, 0], [0, 5], [5, 0], [0, -5], [-5, 0], [3, 3], [-3, 3], [3, -3], [6, 3], [3, 6], [6, -3], [-3, 6], [1, 1], [1, 0], [-1, 0],
    [2, -3], [-3, -2], [240, 46], [-240, 46], [46, 240], [7, 7], [12, 18], [18, -12]] as [$x, $y]) {
    call('gmp_gcdext', $x, $y);
}
// binomial edges
foreach ([[10, 5], [-1, 5], [-2, 6], [5, 7], [0, 0], [0, 1], ['100000000000000000000', 2], ['-100000000000000000000', 3],
    [1000, 500], [10, PHP_INT_MAX], [-10, 3], [7, 0]] as [$n, $k]) call('gmp_binomial', $n, $k);
// fact
foreach ([0, 1, 2, 3, 20, 21, 25, 100, 500, 1000, 3000] as $n) call('gmp_fact', $n);

// ------------------------------------------------------------- operators

$operand_kinds = [5, -5, 0, PHP_INT_MAX, PHP_INT_MIN, 2.0, 2.5, -2.5, 1e30, NAN, INF, true, false, null, '3', ' 3', '0x10',
    '010', '0b11', '-0x1f', '3.5', 'abc', '', [], new stdClass, new T, $RES, g(3), g(-3), g('18446744073709551616')];
$ops = ['+', '-', '*', '/', '%', '**', '&', '|', '^', '<<', '>>', '<=>', '==', '!=', '<', '<=', '>', '>='];
foreach ([g(42), g(-42), g(0), g('100000000000000000000')] as $a) {
    foreach ($operand_kinds as $b) {
        foreach ($ops as $o) {
            // A huge exponent or shift makes GMP abort.
            $huge = $b === PHP_INT_MAX || $b === 1e30;
            if (!($huge && ($o === '**' || $o === '<<'))) op($o, $a, $b);
            if (!(gmp_cmp($a, 1000) > 0 && ($o === '**' || $o === '<<'))) op($o, $b, $a);
        }
    }
}
foreach ([0, 1, 2, 3, 63, 64, 65, 127, 1000, -1, PHP_INT_MIN] as $n) {
    foreach ([g(1), g(-1), g(3), g(-3), g(0), g('-18446744073709551617'), g($big[20])] as $a) {
        op('<<', $a, $n);
        op('>>', $a, $n);
        if ($n <= 1000) op('**', $a, $n);
        op('<<', $a, g($n));
        op('>>', $a, g($n));
    }
}
foreach (['18446744073709551616', '18446744073709551617', '-18446744073709551616', '9223372036854775808', '-9223372036854775809'] as $n) {
    op('**', g(2), g($n));
    op('<<', g(2), g($n));
    op('>>', g(2), g($n));
    op('**', g(2), $n);
    op('>>', g(-2), $n);
}

// ---------------------------------------------------------- serialization

foreach (['O:3:"GMP":1:{i:0;s:2:"2a";}', 'O:3:"GMP":1:{i:0;s:4:"0x2a";}', 'O:3:"GMP":1:{i:0;s:2:"2A";}',
    'O:3:"GMP":1:{i:0;s:3:"-2a";}', 'O:3:"GMP":1:{i:0;s:3:" 2a";}', 'O:3:"GMP":1:{i:0;s:0:"";}',
    'O:3:"GMP":1:{i:0;s:1:"g";}', 'O:3:"GMP":1:{i:0;i:5;}', 'O:3:"GMP":1:{i:0;d:5;}', 'O:3:"GMP":1:{i:0;N;}',
    'O:3:"GMP":1:{i:0;b:1;}', 'O:3:"GMP":0:{}', 'O:3:"GMP":1:{i:1;s:1:"5";}', 'O:3:"GMP":1:{s:3:"num";s:1:"5";}',
    'O:3:"GMP":2:{i:0;s:1:"5";i:1;a:0:{}}', 'O:3:"GMP":2:{i:0;s:1:"5";i:1;i:0;}', 'O:3:"GMP":2:{i:0;s:1:"5";i:1;s:0:"";}',
    'O:3:"GMP":3:{i:0;s:1:"5";i:1;a:0:{}i:2;i:9;}', 'O:3:"GMP":2:{i:1;a:0:{}i:0;s:1:"7";}',
    'O:3:"GMP":1:{i:0;s:33:"ffffffffffffffffffffffffffffffff0";}', 'O:3:"GMP":1:{i:0;s:5:"1 2 3";}',
    'C:3:"GMP":15:{s:2:"42";a:0:{}}', 'C:3:"GMP":16:{s:3:"-42";a:0:{}}', 'C:3:"GMP":16:{s:3:" 42";a:0:{}}',
    'C:3:"GMP":17:{s:4:"0x2a";a:0:{}}', 'C:3:"GMP":16:{s:3:"042";a:0:{}}', 'C:3:"GMP":11:{i:42;a:0:{}}',
    'C:3:"GMP":9:{s:2:"42";}', 'C:3:"GMP":13:{s:2:"42";i:0;}', 'C:3:"GMP":0:{}', 'C:3:"GMP":13:{s:0:"";a:0:{}}',
    'C:3:"GMP":16:{s:3:"4 2";a:0:{}}', 'C:3:"GMP":15:{s:2:"2a";a:0:{}}'] as $str) {
    run('unserialize', [$str], fn($s) => unserialize($s));
}
foreach (['2a', '0x2a', '-ff', ' 1', '', 'g', 'FFFF'] as $str) {
    run('__unserialize', [$str], function ($s) { $g = gmp_init(1); $g->__unserialize([$s]); return $g; });
}
foreach ([5, null, 1.5, true, []] as $v) {
    run('__unserialize', [$v], function ($s) { $g = gmp_init(1); $g->__unserialize([$s]); return $g; });
}
run('__unserialize_empty', [], function () { $g = gmp_init(1); $g->__unserialize([]); return $g; });
run('__unserialize_props_bad', [], function () { $g = gmp_init(1); $g->__unserialize(['5', 3]); return $g; });

// ----------------------------------------------------------------- random

foreach ([0, 1, -1, 100, 12345, PHP_INT_MAX, PHP_INT_MIN, '18446744073709551616', '-18446744073709551616', $big[25], '0x10'] as $seed) {
    call('gmp_random_seed', $seed);
    foreach ([1, 2, 7, 10, 31, 32, 33, 63, 64, 65, 100, 1000] as $b) call('gmp_random_bits', $b);
    foreach ([[0, 1], [0, 2], [-1, 1], [0, 10], [0, 255], [0, 256], [-10000, 10000], [0, PHP_INT_MAX],
        [PHP_INT_MIN, PHP_INT_MAX], [0, '18446744073709551616'], ['-1' . str_repeat('0', 40), '1' . str_repeat('0', 40)],
        [5, 6], [1, '340282366920938463463374607431768211456']] as [$lo, $hi]) {
        call('gmp_random_range', $lo, $hi);
    }
}
call('gmp_random_seed', 7);
for ($i = 0; $i < 300; $i++) {
    if (mt_rand(0, 1)) call('gmp_random_bits', mt_rand(1, 300));
    else call('gmp_random_range', -mt_rand(0, 1000000), mt_rand(1, 1000000000));
}

// ---------------------------------------------------------- large operands
//
// Operands of thousands of bits are not written out: R<seed>:<bits>:<neg>
// stands for gmp_random_seed(seed) then gmp_random_bits(bits) (negated
// when neg is 1), and L<seed>:<length>:<base> for a digit string from the
// LCG in lcg_digits(); the replay rebuilds both.

class R {
    public function __construct(public int $seed, public int $bits, public int $neg) {}
    public function value(): GMP {
        gmp_random_seed($this->seed);
        $v = gmp_random_bits($this->bits);
        return $this->neg ? gmp_neg($v) : $v;
    }
    public function token(): string { return "R{$this->seed}:{$this->bits}:{$this->neg}"; }
}

class L {
    public function __construct(public int $seed, public int $len, public int $base) {}
    public function value(): string {
        $digits = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
        $x = $this->seed;
        $s = '';
        for ($i = 0; $i < $this->len; $i++) {
            $x = ($x * 1103515245 + 12345) % 2147483648;
            $s .= $digits[1 + ($x >> 16) % ($this->base - 1)];
        }
        return $s;
    }
    public function token(): string { return "L{$this->seed}:{$this->len}:{$this->base}"; }
}

function big_call($fn, ...$args) {
    global $notes;
    $vals = array_map(fn($a) => $a instanceof R || $a instanceof L ? $a->value() : $a, $args);
    $notes = [];
    try {
        $out = enc_out($fn(...$vals));
    } catch (Throwable $e) {
        $out = 'E' . get_class($e) . ':' . $e->getMessage();
    }
    $line = $fn;
    foreach ($args as $a) $line .= "\t" . ($a instanceof R || $a instanceof L ? $a->token() : enc_arg($a));
    echo $line, "\t=>\t", $out, "\n";
}

$seed = 1000;
$mk = function ($bits, $neg = 0) use (&$seed) { return new R($seed++, $bits, $neg); };
foreach ([3000, 20000, 100000] as $bits) {
    $a = $mk($bits);
    $b = $mk(intdiv($bits, 2) + 17, 1);
    $c = $mk($bits + 64);
    foreach (['gmp_add', 'gmp_sub', 'gmp_mul', 'gmp_and', 'gmp_or', 'gmp_xor', 'gmp_cmp', 'gmp_hamdist'] as $fn) {
        big_call($fn, $a, $b);
        big_call($fn, $a, $c);
    }
    foreach (['gmp_strval', 'gmp_sqrt', 'gmp_sqrtrem', 'gmp_popcount', 'gmp_com', 'gmp_perfect_square', 'gmp_export', 'gmp_intval'] as $fn) {
        big_call($fn, $a);
        big_call($fn, $b);
    }
    foreach ([2, 3, 7, 16, 36, 37, 62, -16, -36] as $base) {
        big_call('gmp_strval', $a, $base);
        big_call('gmp_strval', $b, $base);
    }
    foreach ([3, 7, 100] as $n) {
        big_call('gmp_root', $a, $n);
        big_call('gmp_rootrem', $b, $n);
    }
    foreach ([0, 1, 1000, $bits - 1, $bits + 5] as $i) {
        big_call('gmp_testbit', $b, $i);
        big_call('gmp_scan0', $a, $i);
        big_call('gmp_scan1', $b, $i);
    }
    if ($bits <= 20000) {
        foreach ([GMP_ROUND_ZERO, GMP_ROUND_PLUSINF, GMP_ROUND_MINUSINF] as $m) {
            big_call('gmp_div_qr', $a, $b, $m);
            big_call('gmp_div_qr', $c, $a, $m);
        }
        big_call('gmp_mod', $a, $b);
        big_call('gmp_gcd', $a, $b);
        big_call('gmp_lcm', $a, $b);
        big_call('gmp_invert', $a, $b);
        big_call('gmp_jacobi', $a, $c);
        big_call('gmp_kronecker', $b, $a);
    }
    if ($bits <= 3000) {
        big_call('gmp_gcdext', $a, $b);
        big_call('gmp_powm', $a, $b, $c);
        big_call('gmp_powm', $b, $mk(200), $a);
        big_call('gmp_prob_prime', $a);
        big_call('gmp_nextprime', $mk(600));
    }
}
foreach ([2, 10, 16, 36, 37, 62] as $base) {
    foreach ([500, 5000, 60000] as $len) {
        $l = new L($len + $base, $len, $base);
        big_call('gmp_init', $l, $base);
        big_call('gmp_strval', $l, 10);
    }
}
foreach ([2000, 20000] as $n) big_call('gmp_fact', $n);
big_call('gmp_binomial', 100000, 300);
big_call('gmp_pow', 3, 100000);
big_call('gmp_pow', $mk(1000, 1), 33);
