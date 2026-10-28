<?php
// Observation driver, part 3, for the reference PHP 8.5 (glibc 2.43 iconv):
//   php85 iconv_obs_extra.php > extra.out
// - GB18030 four-byte sequences: "G <first-linear-index> <first-cp> <count>"
//   runs (linear index of b1 b2 b3 b4 as in the GB18030 standard), "GX <index>"
//   for illegal ones, and the encoding of the SMP ("GS" lines).
// - Composition on decode for CP1255, CP1258 and TCVN: "C <charset> <b1>
//   <b2> <cps>" when decoding the byte pair does not give the two characters
//   decoded separately.
set_error_handler(function () { return true; });
function seq4($i) {
    $b4 = $i % 10; $i = intdiv($i, 10);
    $b3 = $i % 126; $i = intdiv($i, 126);
    $b2 = $i % 10; $b1 = intdiv($i, 10);
    return chr(0x81 + $b1) . chr(0x30 + $b2) . chr(0x81 + $b3) . chr(0x30 + $b4);
}
// GB18030 four-byte area: b1 81..FE gives 126*10*126*10 = 1587600 codes.
$runStart = null; $runCp = 0; $runLen = 0;
$flush = function () use (&$runStart, &$runCp, &$runLen) {
    if ($runStart !== null) printf("G %d %X %d\n", $runStart, $runCp, $runLen);
    $runStart = null;
};
for ($blk = 0; $blk < 1587600; $blk += 1260) {
    $in = '';
    for ($i = $blk; $i < $blk + 1260; $i++) $in .= seq4($i);
    $r = iconv('GB18030', 'UTF-32BE', $in);
    $cps = [];
    if ($r !== false && strlen($r) == 1260 * 4) {
        $cps = array_values(unpack('N*', $r));
    } else {
        for ($i = $blk; $i < $blk + 1260; $i++) {
            $x = iconv('GB18030', 'UTF-32BE', seq4($i));
            $cps[] = ($x === false || strlen($x) != 4) ? -1 : unpack('N', $x)[1];
        }
    }
    foreach ($cps as $k => $cp) {
        $i = $blk + $k;
        if ($cp < 0) { $flush(); echo "GX $i\n"; continue; }
        if ($runStart !== null && $cp == $runCp + $runLen) { $runLen++; continue; }
        $flush();
        $runStart = $i; $runCp = $cp; $runLen = 1;
    }
}
$flush();
// Encoding of every code point above the BMP, in blocks.
for ($blk = 0x10000; $blk <= 0x10FFFF; $blk += 0x1000) {
    $in = '';
    for ($cp = $blk; $cp < $blk + 0x1000; $cp++) $in .= pack('N', $cp);
    $r = iconv('UTF-32BE', 'GB18030', $in);
    printf("GS %X %s\n", $blk, $r === false ? 'x' : md5($r) . ' ' . bin2hex(substr($r, 0, 8)));
}
// Composition pairs.
foreach (['CP1255', 'CP1258', 'TCVN'] as $cs) {
    $single = [];
    for ($b = 0; $b < 256; $b++) {
        $r = iconv($cs, 'UTF-32BE', chr($b));
        $single[$b] = ($r === false || strlen($r) != 4) ? null : unpack('N', $r)[1];
    }
    foreach ($single as $b2 => $c2) {
        if ($c2 === null) continue;
        // Combining marks only (Unicode general category Mn / Mc).
        if (!preg_match('/^\p{M}$/u', mb_chr($c2, 'UTF-8'))) continue;
        foreach ($single as $b1 => $c1) {
            if ($c1 === null) continue;
            $r = iconv($cs, 'UTF-32BE', chr($b1) . chr($b2));
            $exp = pack('N', $c1) . pack('N', $c2);
            if ($r === $exp) continue;
            $o = $r === false ? 'x' : implode('+', array_map(fn($c) => sprintf('%X', $c), unpack('N*', $r)));
            printf("C %s %02x %02x %s\n", $cs, $b1, $b2, $o);
        }
    }
    // Two marks after a base: does the composed character compose again?
    foreach ([0x61, 0x41, 0x65, 0x6f, 0x75] as $b1) {
        foreach ($single as $b2 => $c2) {
            if ($c2 === null || !preg_match('/^\p{M}$/u', mb_chr($c2, 'UTF-8'))) continue;
            foreach ($single as $b3 => $c3) {
                if ($c3 === null || !preg_match('/^\p{M}$/u', mb_chr($c3, 'UTF-8'))) continue;
                $r = iconv($cs, 'UTF-32BE', chr($b1) . chr($b2) . chr($b3));
                $o = $r === false ? 'x' : implode('+', array_map(fn($c) => sprintf('%X', $c), unpack('N*', $r)));
                printf("C3 %s %02x %02x %02x %s\n", $cs, $b1, $b2, $b3, $o);
            }
        }
    }
}
