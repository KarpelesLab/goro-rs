<?php
// Array writes in place (sieve of Eratosthenes) and nested loops.
$t = microtime(true);
$n = 5000000;
$flags = array_fill(0, $n + 1, true);
$flags[0] = $flags[1] = false;
for ($i = 2; $i * $i <= $n; $i++) {
    if ($flags[$i]) {
        for ($j = $i * $i; $j <= $n; $j += $i) {
            $flags[$j] = false;
        }
    }
}
$c = 0;
foreach ($flags as $f) {
    if ($f) {
        $c++;
    }
}
printf("%d primes in %.3fs\n", $c, microtime(true) - $t);
