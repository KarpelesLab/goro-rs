<?php
// Closures: creation, calls, captured variables, callables, usort.
$t = hrtime(true);
$check = 0;
$mul = 3;
$f = function ($x) use ($mul) { return $x * $mul; };
$g = fn($x) => $f($x) + 1;
for ($i = 0; $i < 2000000; $i++) { $check += $g($i); }
$counter = 0;
$inc = function () use (&$counter) { $counter++; };
for ($i = 0; $i < 1200000; $i++) { $inc(); }
$check += $counter;
$adders = [];
for ($i = 0; $i < 100000; $i++) { $adders[] = fn($x) => $x + $i; }
for ($k = 0; $k < 4; $k++) { foreach ($adders as $a) { $check += $a(1); } }
$arr = [];
for ($i = 0; $i < 100000; $i++) { $arr[] = ($i * 7919) % 10007; }
usort($arr, fn($a, $b) => $a <=> $b);
$check += $arr[0] + $arr[99999];
$check += array_reduce(range(1, 400000), fn($c, $x) => $c + $x, 0);
$check += call_user_func_array('max', [1, 5, 3]) + call_user_func('strlen', 'abc');
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $check);
