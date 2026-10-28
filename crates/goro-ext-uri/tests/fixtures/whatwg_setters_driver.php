<?php
// Fixture driver: Uri\WhatWg\Url withers on PHP 8.5.
// Input lines: hex(url) \t method \t N | I<int> | S<hex>
require __DIR__ . '/common.php';
$in = fopen($argv[1], 'r');
while (($line = fgets($in)) !== false) {
    $line = rtrim($line, "\n");
    if ($line === '') continue;
    [$hu, $m, $a] = explode("\t", $line);
    $arg = $a === 'N' ? null : ($a[0] === 'I' ? (int)substr($a, 1) : hex2bin(substr($a, 1)));
    $out = "$hu\t$m\t$a\t";
    $u = Uri\WhatWg\Url::parse(hex2bin($hu));
    if ($u === null) { echo $out, "PARSEFAIL\n"; continue; }
    try {
        $v = $u->$m($arg);
        $out .= wurl($v);
    } catch (Throwable $e) {
        $out .= exc($e);
    }
    echo $out, "\n";
}
