<?php
// Observation driver for the iconv tables: run on the reference PHP 8.5
// (glibc 2.43 iconv) as a black box, output goes to the scratch directory and
// is diffed against the public mapping tables by tools/charset/gen_iconv.py.
//
//   php85 iconv_obs_tables.php <mode> <listfile> > out
//
// Each line of the list file is "<charset> [maxdepth] [smp]".
//
// mode "dec": decode every byte sequence. One line per prefix that PHP
//   reports as incomplete: "D <charset> <prefix-hex> e0 e1 ... e255", where
//   each entry is the decoded code points (hex, '+' separated), "i" for an
//   incomplete prefix, or "x" / "x:<hex>" for an illegal sequence, followed
//   by what //IGNORE keeps of it.
// mode "enc": the "dec" dump, then encode every BMP code point (plus planes 2 and the SMP when
//   asked) in blocks of 256 with //IGNORE, then one by one for blocks that
//   differ from the inverse of the decode table. Prints "E <charset> <cp-hex>
//   <bytes-hex|x>" for every code point of a mismatching block.
$GLOBALS['lasterr'] = '';
set_error_handler(function ($no, $msg) { $GLOBALS['lasterr'] = $msg; return true; });
function conv($from, $to, $s) {
    $GLOBALS['lasterr'] = '';
    $r = iconv($from, $to, $s);
    if ($r !== false) return $r;
    $e = $GLOBALS['lasterr'];
    if (strpos($e, 'incomplete') !== false) return 1;
    if (strpos($e, 'illegal') !== false) return 2;
    return 3;
}
function cps($r) {
    $o = [];
    foreach (str_split(bin2hex($r), 8) as $h) $o[] = strtoupper(ltrim($h, '0') ?: '0');
    return implode('+', $o);
}
function dump($cs, $prefix, $depth, $maxdepth, &$inverse) {
    $line = [];
    $sub = [];
    for ($b = 0; $b < 256; $b++) {
        $s = $prefix . chr($b);
        $r = conv($cs, 'UTF-32BE', $s);
        if (is_string($r)) {
            if ($r === '') { $line[] = 'e'; continue; }
            $line[] = cps($r);
            if (strlen($r) == 4) {
                $cp = unpack('N', $r)[1];
                if (!isset($inverse[$cp])) $inverse[$cp] = $s;
            }
        } elseif ($r === 1) {
            $line[] = 'i';
            if ($depth < $maxdepth) $sub[] = $s;
        } else {
            $g = conv($cs, 'UTF-32BE//IGNORE', $s);
            $line[] = is_string($g) ? ($g === '' ? 'x' : 'x:' . cps($g)) : ($g === 1 ? 'x:i' : 'x:x');
        }
    }
    echo "D $cs ", bin2hex($prefix) ?: '-', ' ', implode(' ', $line), "\n";
    foreach ($sub as $s) dump($cs, $s, $depth + 1, $maxdepth, $inverse);
}
$mode = $argv[1];
foreach (file($argv[2], FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) as $spec) {
    $f = explode(' ', $spec);
    $cs = $f[0];
    $maxdepth = (int)($f[1] ?? 3);
    $smp = ($f[2] ?? '') === 'smp';
    $inverse = [];
    if ($mode === 'dec') {
        dump($cs, '', 1, $maxdepth, $inverse);
    } elseif ($mode === 'enc') {
        dump($cs, '', 1, $maxdepth, $inverse);
        $ranges = [[0, 0xFFFF]];
        if ($smp) { $ranges[] = [0x10000, 0x10FFFF]; }
        foreach ($ranges as [$lo, $hi]) {
            for ($blk = $lo; $blk <= $hi; $blk += 256) {
                $in = '';
                $exp = '';
                for ($cp = $blk; $cp < $blk + 256; $cp++) {
                    if ($cp >= 0xD800 && $cp < 0xE000) continue;
                    $in .= pack('N', $cp);
                    if (isset($inverse[$cp])) $exp .= $inverse[$cp];
                }
                $got = conv('UTF-32BE', $cs . '//IGNORE', $in);
                if ($got === $exp) continue;
                for ($cp = $blk; $cp < $blk + 256; $cp++) {
                    if ($cp >= 0xD800 && $cp < 0xE000) continue;
                    $r = conv('UTF-32BE', $cs, pack('N', $cp));
                    $e = $inverse[$cp] ?? null;
                    if (is_string($r) && $r === $e) continue;
                    if (!is_string($r) && $e === null) continue;
                    printf("E %s %X %s\n", $cs, $cp, is_string($r) ? bin2hex($r) : 'x');
                }
            }
        }
    }
}
