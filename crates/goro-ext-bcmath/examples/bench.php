<?php
// The reference side of examples/bcmath_bench.rs: the same operations on
// the same operands (built by the same LCG), timed with hrtime().
// php -d memory_limit=-1 bench.php
function gen($seed, $n) {
    $x = $seed;
    $s = '';
    for ($i = 0; $i < $n; $i++) {
        $x = ($x * 1103515245 + 12345) % 2147483648;
        $d = ($x >> 16) % 10;
        if ($i == 0 && $d == 0) $d = 1;
        $s .= chr(48 + $d);
    }
    return $s;
}
function bench($label, $iters, $f) {
    $f();
    $t = hrtime(true);
    for ($i = 0; $i < $iters; $i++) $f();
    $ns = (hrtime(true) - $t) / $iters;
    printf("%-36s %12.1f ns/op\n", $label, $ns);
}
$a = '12345678901234567890.1234567890';
$b = '98765432109876543210.9876543210';
$n = 1000000;
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { }
printf("%-36s %12.1f ns/op\n", 'empty loop', (hrtime(true) - $t) / $n);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { bcadd($a, $b, 10); }
printf("%-36s %12.1f ns/op\n", 'bcadd 20.10 digits x1e6', (hrtime(true) - $t) / $n);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { bcsub($a, $b, 10); }
printf("%-36s %12.1f ns/op\n", 'bcsub 20.10 digits x1e6', (hrtime(true) - $t) / $n);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { bcmul($a, $b, 10); }
printf("%-36s %12.1f ns/op\n", 'bcmul 20.10 digits x1e6', (hrtime(true) - $t) / $n);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { bcdiv($a, $b, 10); }
printf("%-36s %12.1f ns/op\n", 'bcdiv 20.10 digits x1e6', (hrtime(true) - $t) / $n);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { bccomp($a, $b, 10); }
printf("%-36s %12.1f ns/op\n", 'bccomp 20.10 digits x1e6', (hrtime(true) - $t) / $n);
$x = new BcMath\Number($a);
$y = new BcMath\Number($b);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { $x + $y; }
printf("%-36s %12.1f ns/op\n", 'Number + 20.10 digits x1e6', (hrtime(true) - $t) / $n);

foreach ([100, 300, 1000, 10000, 100000] as $d) {
    $p = gen(1, $d);
    $q = gen(2, $d);
    $h = gen(3, intdiv($d, 2));
    $it = $d >= 100000 ? 1 : ($d >= 10000 ? 5 : ($d >= 1000 ? 200 : 5000));
    bench("bcmul {$d}x{$d}", $it, fn() => bcmul($p, $q));
    bench("bcdiv {$d}/" . intdiv($d, 2) . " scale 0", $it, fn() => bcdiv($p, $h, 0));
    bench("bcdiv {$d}/{$d} scale {$d}", $it, fn() => bcdiv($p, $q, $d));
    bench("bcsqrt {$d} scale 0", $it, fn() => bcsqrt($p, 0));
    bench("bcadd {$d}+{$d}", $it * 20, fn() => bcadd($p, $q));
}
bench('bcpow 7^50000', 1, fn() => bcpow('7', '50000'));
bench('bcpow 1.0001^10000 scale 20', 1, fn() => bcpow('1.0001', '10000', 20));
bench('bcpow 123456789.987654321^1000', 1, fn() => bcpow('123456789.987654321', '1000', 0));
