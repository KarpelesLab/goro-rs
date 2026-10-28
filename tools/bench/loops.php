<?php
// Tight loops: int/float arithmetic and local variable access.
$t = microtime(true);
$s = 0;
$f = 0.0;
for ($i = 0; $i < 20000000; $i++) {
    $s += $i * 3 % 7;
    $f += $i * 0.5;
}
printf("%d %.1f in %.3fs\n", $s, $f, microtime(true) - $t);
