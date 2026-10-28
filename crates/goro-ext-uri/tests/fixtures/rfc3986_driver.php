<?php
// Fixture driver: Uri\Rfc3986\Uri on PHP 8.5.  See gen_rfc3986_cases.py for the ops.
require __DIR__ . '/common.php';
use Uri\Rfc3986\Uri;
use Uri\UriComparisonMode;

function arg($hex) {
    $a = hex2bin($hex);
    if ($a === 'N') return null;
    if ($a[0] === 'I') return (int)substr($a, 1);
    return substr($a, 1);
}

$in = fopen($argv[1], 'r');
while (($line = fgets($in)) !== false) {
    $line = rtrim($line, "\n");
    if ($line === '') continue;
    $f = explode("\t", $line);
    $op = $f[0];
    $out = $line . "\t";
    try {
        switch ($op) {
            case 'P':
                $u = Uri::parse(hex2bin($f[1]));
                $out .= ruri($u);
                if ($u === null) {
                    try { new Uri(hex2bin($f[1])); $out .= "\tNOEXC"; } catch (Throwable $e) { $out .= "\t" . exc($e); }
                }
                break;
            case 'R':
                $b = Uri::parse(hex2bin($f[1]));
                if ($b === null) { $out .= 'BASEFAIL'; break; }
                $r = $b->resolve(hex2bin($f[2]));
                $out .= ruri($r);
                $p = Uri::parse(hex2bin($f[2]), $b);
                $out .= "\t" . ($p === null ? 'NULL' : q($p->toRawString()));
                break;
            case 'E':
                $a = Uri::parse(hex2bin($f[1]));
                $b = Uri::parse(hex2bin($f[2]));
                if ($a === null || $b === null) { $out .= 'PARSEFAIL'; break; }
                $out .= ($a->equals($b) ? 'T' : 'F') . ($a->equals($b, UriComparisonMode::IncludeFragment) ? 'T' : 'F')
                    . ($b->equals($a) ? 'T' : 'F');
                break;
            case 'W':
                $u = Uri::parse(hex2bin($f[1]));
                if ($u === null) { $out .= 'PARSEFAIL'; break; }
                $m = 'with' . ucfirst($f[2] === '75736572696e666f' ? 'userInfo' : hex2bin($f[2]));
                $r = $u->$m(arg($f[3]));
                $out .= ruri($r);
                break;
        }
    } catch (Throwable $e) {
        $out .= exc($e);
    }
    echo $out, "\n";
}
