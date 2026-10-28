<?php
// Recursive calls: function call overhead, int arithmetic, comparisons.
function fib(int $n): int { return $n < 2 ? $n : fib($n - 1) + fib($n - 2); }
$t = microtime(true);
$r = fib(30);
printf("fib(30) = %d in %.3fs\n", $r, microtime(true) - $t);
