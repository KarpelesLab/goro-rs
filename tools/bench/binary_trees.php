<?php
// Allocation-heavy recursion with arrays (binary trees).
function make(int $d): array { return $d == 0 ? [null, null] : [make($d - 1), make($d - 1)]; }
function check(array $t): int { return 1 + ($t[0] === null ? 0 : check($t[0]) + check($t[1])); }
$t = microtime(true);
$total = 0;
for ($d = 4; $d <= 16; $d += 2) {
    $iters = 1 << (16 - $d + 4);
    for ($i = 0; $i < $iters; $i++) {
        $total += check(make($d));
    }
}
printf("%d in %.3fs\n", $total, microtime(true) - $t);
