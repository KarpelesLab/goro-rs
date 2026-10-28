<?php
// Observation driver for //TRANSLIT on the reference PHP 8.5 (glibc 2.43
// iconv, C.UTF-8 locale): for every code point that the target charset can't
// encode directly, print what //TRANSLIT turns it into.
//   php85 iconv_obs_translit.php > translit.out
// Lines: "T <target> <cp-hex> <result-hex>" ("-" for an empty result, "x" for
// a failure). For the ASCII target only results other than "?" are printed;
// for other targets only results that differ from the ASCII one.
set_error_handler(function () { return true; });
$targets = ['ASCII', 'ISO-8859-1', 'ISO-8859-2', 'ISO-8859-15', 'CP1252', 'ISO-8859-5',
    'KOI8-R', 'ISO-8859-7', 'SJIS', 'EUC-KR', 'GBK', 'EUC-JP', 'BIG5', 'CP1250', 'CP1251'];
$ascii = [];
foreach ($targets as $t) {
    for ($cp = 0x80; $cp <= 0x10FFFF; $cp++) {
        if ($cp == 0xD800) $cp = 0xE000;
        if ($cp == 0x30000) $cp = 0xE0000;
        if ($cp == 0xE1000) break;
        $u = pack('N', $cp);
        if (iconv('UTF-32BE', $t, $u) !== false) continue;
        $r = iconv('UTF-32BE', $t . '//TRANSLIT', $u);
        $h = $r === false ? 'x' : ($r === '' ? '-' : bin2hex($r));
        if ($t === 'ASCII') {
            $ascii[$cp] = $h;
            if ($h !== '3f') echo "T $t ", dechex($cp), " $h\n";
        } else {
            $a = $ascii[$cp] ?? 'none';
            if ($a !== $h) {
                // A result that is the ASCII one re-encoded is not interesting.
                if ($a !== 'x' && $a !== 'none' && $r !== false) {
                    $re = iconv('ASCII', $t, $a === '-' ? '' : hex2bin($a));
                    if ($re === $r) continue;
                }
                echo "T $t ", dechex($cp), " $h\n";
            }
        }
    }
}
