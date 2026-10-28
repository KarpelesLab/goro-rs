<?php
// Float-heavy nested loops.
$t = microtime(true);
$w = 600;
$h = 400;
$count = 0;
for ($y = 0; $y < $h; $y++) {
    $ci = ($y / $h) * 2.0 - 1.0;
    for ($x = 0; $x < $w; $x++) {
        $cr = ($x / $w) * 3.0 - 2.0;
        $zr = 0.0;
        $zi = 0.0;
        $k = 0;
        while ($k < 50 && $zr * $zr + $zi * $zi < 4.0) {
            $t2 = $zr * $zr - $zi * $zi + $cr;
            $zi = 2.0 * $zr * $zi + $ci;
            $zr = $t2;
            $k++;
        }
        if ($k == 50) {
            $count++;
        }
    }
}
printf("%d in %.3fs\n", $count, microtime(true) - $t);
