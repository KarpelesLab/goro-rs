<?php
// Float arithmetic in nested loops.
function mandel($w, $h, $iter) {
    $count = 0;
    for ($y = 0; $y < $h; $y++) {
        $ci = 2.0 * $y / $h - 1.0;
        for ($x = 0; $x < $w; $x++) {
            $cr = 2.0 * $x / $w - 1.5;
            $zr = 0.0; $zi = 0.0;
            $i = 0;
            while ($i < $iter) {
                $tr = $zr * $zr - $zi * $zi + $cr;
                $zi = 2.0 * $zr * $zi + $ci;
                $zr = $tr;
                if ($zr * $zr + $zi * $zi > 4.0) break;
                $i++;
            }
            if ($i == $iter) $count++;
        }
    }
    return $count;
}
$t = hrtime(true);
$r = mandel(600, 450, 100);
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $r);
