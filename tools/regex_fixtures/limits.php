<?php
// For each case (hex pattern TAB hex subject), find the smallest
// pcre.backtrack_limit and pcre.recursion_limit with which preg_match
// does not fail (pcre.jit must be 0).
function minlimit($ini, $p, $s) {
    $lo = 1; $hi = 20000000;
    ini_set($ini, $hi);
    if (@preg_match($p, $s) === false) return -1;
    while ($lo < $hi) {
        $mid = intdiv($lo + $hi, 2);
        ini_set($ini, $mid);
        if (@preg_match($p, $s) === false) $lo = $mid + 1; else $hi = $mid;
    }
    return $lo;
}
foreach (file($argv[1], FILE_IGNORE_NEW_LINES) as $l) {
    [$p, $s] = explode("\t", $l);
    $p = hex2bin($p); $s = hex2bin($s);
    ini_set('pcre.recursion_limit', 20000000);
    $m = minlimit('pcre.backtrack_limit', $p, $s);
    ini_set('pcre.backtrack_limit', 20000000);
    $d = minlimit('pcre.recursion_limit', $p, $s);
    echo bin2hex($p), "\t", bin2hex($s), "\t", $m, " ", $d, "\n";
}
