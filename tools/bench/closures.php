<?php
// Closures, callables, generators.
$t = microtime(true);
$mul = 3;
$f = function ($x) use ($mul) { return $x * $mul; };
$g = fn($x) => $x + $mul;
$s = 0;
for ($i = 0; $i < 2000000; $i++) {
    $s += $f($i) + $g($i);
}
function gen($n)
{
    for ($i = 0; $i < $n; $i++) {
        yield $i => $i * 2;
    }
}
foreach (gen(2000000) as $k => $v) {
    $s += $v - $k;
}
$s += array_sum(array_map('abs', range(-100000, 100000)));
$arr = range(1, 100000);
usort($arr, fn($a, $b) => $b <=> $a);
printf("%d %d in %.3fs\n", $s, $arr[0], microtime(true) - $t);
