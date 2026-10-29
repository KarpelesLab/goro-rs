<?php
// Packed arrays: fill, iterate, sort, array functions.
$t = hrtime(true);
$check = 0;
for ($round = 0; $round < 10; $round++) {
    $a = [];
    for ($i = 0; $i < 200000; $i++) { $a[] = ($i * 7919) % 100003; }
    $sum = 0;
    foreach ($a as $v) { $sum += $v; }
    for ($i = 0, $n = count($a); $i < $n; $i++) { $sum += $a[$i]; }
    sort($a);
    $b = array_map(fn($x) => $x * 2, array_slice($a, 0, 50000));
    $c = array_filter($b, fn($x) => $x % 3 == 0);
    $check += $sum + count($c) + $a[100] + array_sum($b) + in_array(99999, $a);
    $r = array_reverse($a);
    $check += $r[0];
}
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $check);
