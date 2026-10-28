<?php
// Observation driver for //TRANSLIT on the reference PHP 8.5 (glibc 2.43
// iconv, C.UTF-8 locale): for every code point that the target charset can't
// encode directly, print what //TRANSLIT turns it into.
//   php85 iconv_obs_translit.php charsets.txt > translit.out
// The targets are ASCII, UCS-2 and every table charset of
// tools/charset/iconv-data/charsets.txt.
// Lines: "T <target> <cp-hex> <result-hex>" ("-" for an empty result, "x" for
// a failure). For the ASCII target only results other than "?" are printed;
// for other targets only results that differ from the ASCII one.
set_error_handler(function () { return true; });
$targets = ['ASCII', 'UCS-2'];
foreach (file($argv[1], FILE_IGNORE_NEW_LINES) as $line) {
    if ($line === '' || $line[0] === '#') continue;
    $p = explode(' ', $line);
    if ($p[1] === 'table' || $p[1] === 'compose' || $p[0] === 'GB18030') $targets[] = $p[0];
}
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
