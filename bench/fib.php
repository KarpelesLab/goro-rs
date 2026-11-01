<?php
// Function calls and recursion.
function fib($n) {
    return $n < 2 ? $n : fib($n - 1) + fib($n - 2);
}
$t = hrtime(true);
$r = fib(34);
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $r);
