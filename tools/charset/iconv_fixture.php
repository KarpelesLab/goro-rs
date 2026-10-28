<?php
// Fixture driver for crates/goro-charset/tests/fixtures/iconv_tables.txt:
// per-charset CRC-32 checksums of PHP 8.5's (glibc 2.43) iconv behaviour,
// which the goro-charset tests recompute with the Rust implementation.
//
//   php85 iconv_fixture.php tools/charset/iconv-data/charsets.txt > iconv_tables.txt
//
// For each charset (first field of each non-skip line of charsets.txt):
//   DEC  <cs> <crc>: every byte after every prefix that is incomplete (one
//        byte, or two when at most 128 single bytes are incomplete), decoded to UTF-32BE: "hex" | "e" (empty) | "x" (illegal)
//        | "i" (incomplete), joined with ','.
//   DECI <cs> <crc>: the same sequences followed by "A", decoded with //IGNORE.
//   ENC  <cs> <crc>: every BMP code point but the surrogates, in blocks of
//        4096, encoded from UTF-32BE with //IGNORE.
//   ENCS <cs> <crc>: the same for planes 1, 2, 14 and 15 (blocks of 4096:
//        PHP fails with //IGNORE when more than about 8160 characters follow
//        an ignored one, see the iconv module documentation).
// And for some targets, //TRANSLIT//IGNORE of blocks of 4096 code points:
//   TR <cs> <first-cp-hex> <crc>
$GLOBALS['lasterr'] = '';
set_error_handler(function ($no, $msg) { $GLOBALS['lasterr'] = $msg; return true; });
function k($f, $t, $s) {
    $GLOBALS['lasterr'] = '';
    $r = iconv($f, $t, $s);
    if ($r !== false) return $r === '' ? 'e' : bin2hex($r);
    $e = $GLOBALS['lasterr'];
    return strpos($e, 'incomplete') !== false ? 'i' : (strpos($e, 'illegal') !== false ? 'x' : 'W');
}
function block($lo, $hi) {
    $s = '';
    for ($cp = $lo; $cp < $hi; $cp++) {
        if ($cp >= 0xD800 && $cp < 0xE000) continue;
        $s .= pack('N', $cp);
    }
    return $s;
}
$list = [];
foreach (file($argv[1], FILE_IGNORE_NEW_LINES) as $line) {
    if ($line === '' || $line[0] === '#') continue;
    $p = explode(' ', $line);
    if ($p[1] !== 'skip') $list[] = $p[0];
}
$blocks = [];
for ($b = 0; $b < 0x10000; $b += 0x1000) $blocks[] = block($b, $b + 0x1000);
$planes = [];
foreach ([1, 2, 14, 15] as $p) for ($b = 0; $b < 0x10000; $b += 0x1000) $planes[] = block(($p << 16) + $b, ($p << 16) + $b + 0x1000);
foreach ($list as $cs) {
    $dec = [];
    $deci = [];
    $queue = [''];
    $l1 = 0;
    for ($q = 0; $q < count($queue); $q++) {
        $pre = $queue[$q];
        if ($q == 1) $l1 = count($queue) - 1;
        for ($b = 0; $b < 256; $b++) {
            $s = $pre . chr($b);
            $r = k($cs, 'UTF-32BE', $s);
            $dec[] = $r;
            $deci[] = k($cs, 'UTF-32BE//IGNORE', $s . 'A');
            if ($r === 'i' && (strlen($s) == 1 || (strlen($s) == 2 && $l1 <= 128))) $queue[] = $s;
        }
    }
    printf("DEC %s %08x\n", $cs, crc32(implode(',', $dec)));
    printf("DECI %s %08x\n", $cs, crc32(implode(',', $deci)));
    $enc = [];
    foreach ($blocks as $blk) $enc[] = k('UTF-32BE', $cs . '//IGNORE', $blk);
    printf("ENC %s %08x\n", $cs, crc32(implode(',', $enc)));
    $enc = [];
    foreach ($planes as $blk) $enc[] = k('UTF-32BE', $cs . '//IGNORE', $blk);
    printf("ENCS %s %08x\n", $cs, crc32(implode(',', $enc)));
}
foreach (['ASCII', 'ISO-8859-1', 'ISO-8859-2', 'ISO-8859-15', 'CP1252', 'ISO-8859-5', 'KOI8-R',
          'ISO-8859-7', 'SHIFT_JIS', 'EUC-KR', 'GBK', 'EUC-JP', 'BIG5', 'CP1250', 'CP1251',
          'CP1253', 'ISO-8859-9', 'KOI8-U', 'CP437', 'ISO-2022-JP', 'UCS-2', 'EBCDIC-US', 'CP1258'] as $cs) {
    for ($b = 0; $b < 0x30000; $b += 0x1000) {
        printf("TR %s %X %08x\n", $cs, $b, crc32(k('UTF-32BE', $cs . '//TRANSLIT//IGNORE', block(max($b, 0x80), $b + 0x1000))));
    }
    printf("TR %s %X %08x\n", $cs, 0xE0000, crc32(k('UTF-32BE', $cs . '//TRANSLIT//IGNORE', block(0xE0000, 0xE1000))));
}
