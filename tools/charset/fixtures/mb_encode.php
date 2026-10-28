<?php
// Exhaustive encode-direction fixture: for each encoding, each block of 256
// code points (as one UTF-32BE string, U+0000..U+10FFFF) is converted with
// substitute character '%'. Per line: crc32 of illegal_chars delta . ':' . output.
// Blocks of only-substitutes (output equal to the all-'%' result) are skipped
// and summarised by an "empty" line listing their count.
error_reporting(E_ALL & ~E_DEPRECATED);
mb_substitute_character(0x25);
$dumpdir = $argv[1] ?? null;
$encs = $argv[2] ?? null;
$encs = $encs ? explode(',', $encs) : mb_list_encodings();
foreach ($encs as $e) {
    $dump = $dumpdir ? fopen("$dumpdir/" . str_replace('#', '_', $e) . ".enc", 'w') : null;
    $pct = mb_convert_encoding(str_repeat("\x00\x00\x00%", 256), $e, 'UTF-32BE');
    $skipped = 0;
    for ($blk = 0; $blk < 0x1100; $blk++) {
        $s = '';
        for ($i = 0; $i < 256; $i++) $s .= pack('N', $blk * 256 + $i);
        $ill = mb_get_info('illegal_chars');
        $out = mb_convert_encoding($s, $e, 'UTF-32BE');
        $d = mb_get_info('illegal_chars') - $ill;
        if ($out === $pct && $d === 256) { $skipped++; continue; }
        printf("%s %04x %08x\n", $e, $blk, crc32("$d:$out"));
        if ($dump) fwrite($dump, sprintf("%04x %d %s\n", $blk, $d, bin2hex($out)));
    }
    printf("%s empty %d\n", $e, $skipped);
    if ($dump) fclose($dump);
}
