<?php
// Per-code-point IDNA behaviour of Uri\WhatWg\Url (lexbor) on PHP 8.5:
// prints <cp> <ascii host of http://a<cp>b/ or F> <ascii host of the xn-- form or F>
$in = fopen($argv[1], 'r');
$out = fopen('php://stdout', 'w');
while (($line = fgets($in)) !== false) {
    [$cp, $u, $a] = explode("\t", rtrim($line, "\n"));
    $x = Uri\WhatWg\Url::parse('http://' . hex2bin($u) . '/');
    $y = Uri\WhatWg\Url::parse('http://' . hex2bin($a) . '/');
    fwrite($out, $cp . "\t" . ($x === null ? 'F' : $x->getAsciiHost()) . "\t" . ($y === null ? 'F' : $y->getAsciiHost()) . "\n");
}
