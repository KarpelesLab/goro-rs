<?php
// Packed and hash arrays: append, index, foreach, sort, array functions.
$t = microtime(true);
$a = [];
for ($i = 0; $i < 2000000; $i++) {
    $a[] = $i * 2;
}
$sum = 0;
foreach ($a as $v) {
    $sum += $v;
}
for ($i = 0; $i < 2000000; $i++) {
    $sum += $a[$i];
}
$h = [];
for ($i = 0; $i < 500000; $i++) {
    $h["k$i"] = $i;
}
$c = 0;
for ($i = 0; $i < 500000; $i++) {
    if (isset($h["k$i"])) {
        $c++;
    }
}
$b = range(1, 300000);
mt_srand(42);
shuffle($b);
sort($b);
$m = array_map(fn($x) => $x + 1, $b);
$f = array_filter($m, fn($x) => $x % 2 == 0);
printf("%d %d %d %d in %.3fs\n", $sum, $c, count($f), array_sum($f), microtime(true) - $t);
