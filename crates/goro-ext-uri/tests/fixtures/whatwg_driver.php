<?php
// Fixture driver: WHATWG URL parsing on PHP 8.5 (Uri\WhatWg\Url).
// Input lines: tag \t hex(input) \t hex(base)|-
require __DIR__ . '/common.php';
$in = fopen($argv[1], 'r');
while (($line = fgets($in)) !== false) {
    $line = rtrim($line, "\n");
    if ($line === '') continue;
    [$tag, $hi, $hb] = explode("\t", $line);
    $input = hex2bin($hi);
    $out = "$tag\t$hi\t$hb\t";
    $base = null;
    if ($hb !== '-') {
        $base = Uri\WhatWg\Url::parse(hex2bin($hb));
        if ($base === null) {
            echo $out, "BASEFAIL\n";
            continue;
        }
    }
    $errors = [];
    $u = Uri\WhatWg\Url::parse($input, $base, $errors);
    $out .= wurl($u) . "\t" . errs($errors);
    if ($u === null) {
        try {
            new Uri\WhatWg\Url($input, $base);
            $out .= "\tNOEXC";
        } catch (Throwable $e) {
            $out .= "\t" . exc($e);
        }
    }
    echo $out, "\n";
}
