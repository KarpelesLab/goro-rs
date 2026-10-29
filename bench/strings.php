<?php
// String building and the common string functions.
$t = hrtime(true);
$check = 0;
for ($round = 0; $round < 10; $round++) {
    $s = '';
    for ($i = 0; $i < 200000; $i++) { $s .= 'x' . $i . ','; }
    $parts = explode(',', $s);
    $j = implode(';', $parts);
    $r = str_replace(['x1', ';'], ['Y', '|'], $j);
    $f = '';
    for ($i = 0; $i < 50000; $i++) { $f .= sprintf("%05d:%s:%.2f|", $i, 'ab', $i / 7); }
    $u = 0;
    for ($i = 0; $i < 100000; $i++) {
        $w = "item number $i";
        $u += strlen(strtoupper($w)) + strpos($w, 'n') + (substr($w, 5, 3) === 'num' ? 1 : 0);
        $u += strlen(trim("  $w  ")) + ord($w[0]);
    }
    $check += strlen($s) + count($parts) + strlen($r) + strlen($f) + $u + crc32($r);
}
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $check);
