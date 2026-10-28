<?php
// The reference side of examples/gmp_bench.rs: the same operations on the
// same operands (drawn with gmp_random_bits() after gmp_random_seed(), which
// goro-ext-gmp reproduces), timed with hrtime(), best of five runs.
// php -d memory_limit=-1 bench.php [filter]
$filter = $argv[1] ?? '';
function bench($label, $iters, $f) {
    global $filter;
    if ($filter !== '' && !str_contains($label, $filter)) return;
    $f();
    $best = PHP_FLOAT_MAX;
    for ($r = 0; $r < 5; $r++) {
        $t = hrtime(true);
        for ($i = 0; $i < $iters; $i++) $f();
        $best = min($best, (hrtime(true) - $t) / $iters);
    }
    printf("%-36s %14.1f ns/op\n", $label, $best);
}
function val($seed, $bits) {
    gmp_random_seed($seed);
    return gmp_random_bits($bits);
}

// Many small operations (64-bit operands). The loops call through closures
// as the Rust side does; "empty" is the loop and call overhead alone.
$a = val(1, 63);
$b = val(2, 62);
$c = val(3, 40);
$m = gmp_nextprime(val(4, 61));
$n = 100000;
bench('small: empty closure', $n, fn() => null);
bench('small: gmp_add', $n, fn() => gmp_add($a, $b));
bench('small: gmp_mul', $n, fn() => gmp_mul($a, $b));
bench('small: gmp_div_qr', $n, fn() => gmp_div_qr($a, $c));
bench('small: $a * $b + $c', $n, fn() => $a * $b + $c);
bench('small: gmp_cmp', $n, fn() => gmp_cmp($a, $b));
bench('small: gmp_strval', $n, fn() => gmp_strval($a));
bench('small: gmp_init string', $n, fn() => gmp_init('12345678901234567890'));
bench('small: gmp_intval', $n, fn() => gmp_intval($b));
bench('small: gmp_powm 61-bit', $n / 10, fn() => gmp_powm($a, $b, $m));
bench('small: gmp_gcd', $n, fn() => gmp_gcd($a, $b));
bench('small: gmp_prob_prime 61-bit', $n / 10, fn() => gmp_prob_prime($m));
bench('small: gmp_nextprime 63-bit', $n / 100, fn() => gmp_nextprime($a));

foreach ([1000, 10000, 100000, 1000000] as $bits) {
    $x = val(10, $bits);
    $y = val(11, $bits);
    $z = val(12, 2 * $bits);
    $s = gmp_strval($x);
    $it = max(1, intdiv(2000000, $bits));
    bench("mul {$bits}x{$bits}", $it, fn() => gmp_mul($x, $y));
    bench("div_q " . (2 * $bits) . "/{$bits}", $it, fn() => gmp_div_q($z, $y));
    bench("strval {$bits} bits", max(1, intdiv($it, 10)), fn() => gmp_strval($x));
    bench("init decimal {$bits} bits", max(1, intdiv($it, 10)), fn() => gmp_init($s));
    bench("sqrt " . (2 * $bits) . " bits", max(1, intdiv($it, 4)), fn() => gmp_sqrt($z));
    if ($bits <= 100000) bench("gcd {$bits}x{$bits}", max(1, intdiv($it, 10)), fn() => gmp_gcd($x, $y));
}
foreach ([1024, 2048, 4096, 8192] as $bits) {
    $x = val(20, $bits);
    $e = val(21, $bits);
    $mod = gmp_setbit_ret(val(22, $bits));
    bench("powm {$bits}", max(1, intdiv(200000, $bits * $bits / 1024)), fn() => gmp_powm($x, $e, $mod));
}
function gmp_setbit_ret($v) { gmp_setbit($v, 0); return $v; }
