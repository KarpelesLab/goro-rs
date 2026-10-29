<?php
// Associative arrays: string and int keys, isset, unset, ksort.
$t = hrtime(true);
$check = 0;
for ($round = 0; $round < 10; $round++) {
    $h = [];
    for ($i = 0; $i < 200000; $i++) { $h["key_" . $i] = $i; }
    $hits = 0;
    for ($i = 0; $i < 400000; $i += 3) { if (isset($h["key_" . $i])) $hits++; }
    foreach ($h as $k => $v) { if ($v % 10 == 0) unset($h[$k]); }
    $m = [];
    for ($i = 0; $i < 200000; $i++) { $m[$i * 31 % 65536] = ($m[$i * 31 % 65536] ?? 0) + 1; }
    ksort($m);
    $words = [];
    foreach (['alpha', 'beta', 'gamma', 'delta', 'eps'] as $w) {
        for ($i = 0; $i < 20000; $i++) { $words[$w . ($i % 500)] = ($words[$w . ($i % 500)] ?? 0) + 1; }
    }
    $check += $hits + count($h) + count($m) + count($words) + array_key_first($m) + count(array_keys($h));
}
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $check);
