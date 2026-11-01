<?php
// Integer arithmetic and plain loops.
function loops($n) {
    $s = 0;
    for ($i = 0; $i < $n; $i++) {
        $s = ($s + $i * 7) % 1000003;
        $s ^= $i & 0xff;
        if ($s > 500000) { $s -= 3; } else { $s += 1; }
    }
    $j = 0; $k = 0;
    while ($j < $n) { $k += $j >> 3; $j += 2; }
    return $s + $k;
}
$t = hrtime(true);
$r = loops(20000000);
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $r);
