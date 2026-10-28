<?php
// String building and common string functions.
$t = microtime(true);
$s = '';
for ($i = 0; $i < 1000000; $i++) {
    $s .= 'x' . $i;
}
$n = 0;
for ($i = 0; $i < 300000; $i++) {
    $p = str_repeat('ab', 10) . $i;
    $n += strlen(strtoupper($p)) + strpos($p, 'b') + substr_count($p, 'a');
    $q = str_replace('a', 'zz', $p);
    $n += strlen(sprintf('%s-%05d', $q, $i));
}
$parts = explode(',', implode(',', range(1, 200000)));
printf("%d %d %d in %.3fs\n", strlen($s), $n, count($parts), microtime(true) - $t);
