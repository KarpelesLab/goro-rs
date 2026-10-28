<?php
// Per-code-point Unicode operations: for each block of 256 code points,
// crc32 of the results of mb_strwidth and every mb_convert_case mode on each
// code point alone (UTF-8). Blocks where every result is the identity (width
// 1, case unchanged) are skipped and counted on the last line.
// Usage: php mb_unicode.php [DUMPFILE]
$modes = [MB_CASE_UPPER, MB_CASE_LOWER, MB_CASE_TITLE, MB_CASE_FOLD,
    MB_CASE_UPPER_SIMPLE, MB_CASE_LOWER_SIMPLE, MB_CASE_TITLE_SIMPLE, MB_CASE_FOLD_SIMPLE];
$dump = isset($argv[1]) ? fopen($argv[1], 'w') : null;
$skipped = 0;
for ($blk = 0; $blk < 0x1100; $blk++) {
    $r = '';
    $identity = true;
    for ($i = 0; $i < 256; $i++) {
        $cp = $blk * 256 + $i;
        if ($cp >= 0xD800 && $cp <= 0xDFFF) continue;
        $c = mb_chr($cp, 'UTF-8');
        $w = mb_strwidth($c, 'UTF-8');
        $line = $w;
        if ($w != 1) $identity = false;
        foreach ($modes as $m) {
            $o = mb_convert_case($c, $m, 'UTF-8');
            if ($o !== $c) { $identity = false; $line .= ':' . bin2hex($o); } else { $line .= ':='; }
        }
        $r .= $line . ',';
    }
    if ($identity) { $skipped++; continue; }
    printf("%04x %08x\n", $blk, crc32($r));
    if ($dump) fwrite($dump, sprintf("%04x %s\n", $blk, $r));
}
printf("skipped %d\n", $skipped);
