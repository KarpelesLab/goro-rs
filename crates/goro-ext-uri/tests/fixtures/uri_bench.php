<?php
// 1e6 parses of typical URLs per class (observed timing on PHP 8.5).
$urls = [
    'https://user:pass@example.com:8080/path/to/file.php?query=1&a=2#frag',
    'https://www.example.com/index.html',
    'http://localhost/',
    'https://example.org/a/b/c/d?x=1',
];
$n = 1000000;
foreach ($urls as $u) {
    $t = hrtime(true);
    for ($i = 0; $i < $n; $i++) { $x = Uri\Rfc3986\Uri::parse($u); }
    $r = (hrtime(true) - $t) / 1e9;
    $t = hrtime(true);
    for ($i = 0; $i < $n; $i++) { $x = Uri\WhatWg\Url::parse($u); }
    $w = (hrtime(true) - $t) / 1e9;
    $t = hrtime(true);
    for ($i = 0; $i < $n; $i++) { $x = $u; }
    $e = (hrtime(true) - $t) / 1e9;
    printf("%-72s rfc3986 %.3fs  whatwg %.3fs  (empty loop %.3fs)\n", $u, $r, $w, $e);
}
$u = Uri\WhatWg\Url::parse($urls[0]);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { $x = $u->toAsciiString(); }
printf("whatwg toAsciiString %.3fs\n", (hrtime(true) - $t) / 1e9);
$u = Uri\Rfc3986\Uri::parse($urls[0]);
$t = hrtime(true);
for ($i = 0; $i < $n; $i++) { $x = $u->toString(); }
printf("rfc3986 toString %.3fs\n", (hrtime(true) - $t) / 1e9);
