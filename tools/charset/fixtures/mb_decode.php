<?php
// Exhaustive decode-direction fixture: for each encoding, every 1- and 2-byte
// string (plus sampled 3/4-byte prefixes) is converted to UTF-32BE with
// substitute character '%'. Per line: crc32 of the records of one prefix.
// Record = check(0/1) . mb_strlen . ':' . illegal_chars delta . ':' . hex(out) . ','
error_reporting(E_ALL & ~E_DEPRECATED);
mb_substitute_character(0x25);
$dumpdir = $argv[1] ?? null;
$encs = $argv[2] ?? null;
$encs = $encs ? explode(',', $encs) : mb_list_encodings();

function rec($s, $e) {
    $ill = mb_get_info('illegal_chars');
    $out = mb_convert_encoding($s, 'UTF-32BE', $e);
    $d = mb_get_info('illegal_chars') - $ill;
    return (mb_check_encoding($s, $e) ? '1' : '0') . mb_strlen($s, $e) . ':' . $d . ':' . bin2hex($out) . ',';
}
function line($e, $key, $strs, $dump) {
    $r = '';
    foreach ($strs as $s) $r .= rec($s, $e);
    printf("%s %s %08x\n", $e, $key, crc32($r));
    if ($dump) fwrite($dump, "$key $r\n");
}
$utf8 = ['UTF-8', 'UTF-8-Mobile#DOCOMO', 'UTF-8-Mobile#KDDI-A', 'UTF-8-Mobile#KDDI-B', 'UTF-8-Mobile#SOFTBANK'];
$euc3 = ['EUC-JP', 'eucJP-win', 'EUC-JP-2004', 'CP51932'];
$gb4 = ['GB18030', 'GB18030-2022'];
$all = range(0, 255);
foreach ($encs as $e) {
    $dump = $dumpdir ? fopen("$dumpdir/" . str_replace('#', '_', $e) . ".dec", 'w') : null;
    $strs = [];
    foreach ($all as $b) $strs[] = chr($b);
    line($e, '1', $strs, $dump);
    foreach ($all as $b1) {
        $strs = [];
        foreach ($all as $b2) $strs[] = chr($b1) . chr($b2);
        line($e, sprintf('%02x', $b1), $strs, $dump);
    }
    if (in_array($e, $utf8)) {
        $b2s = array_merge(range(0x80, 0xBF), [0, 0x7F, 0xC0, 0xFF]);
        for ($b1 = 0xE0; $b1 <= 0xEF; $b1++) foreach ($b2s as $b2) {
            $strs = [];
            foreach ($all as $b3) $strs[] = chr($b1) . chr($b2) . chr($b3);
            line($e, sprintf('%02x%02x', $b1, $b2), $strs, $dump);
        }
        for ($b1 = 0xF0; $b1 <= 0xF4; $b1++) foreach ($b2s as $b2) {
            $strs = [];
            foreach ([0x80, 0x9F, 0xBF, 0x41] as $b3) foreach ($all as $b4) $strs[] = chr($b1) . chr($b2) . chr($b3) . chr($b4);
            line($e, sprintf('%02x%02x', $b1, $b2), $strs, $dump);
        }
    }
    if (in_array($e, $euc3)) {
        foreach ($all as $b2) {
            $strs = [];
            foreach ($all as $b3) $strs[] = "\x8F" . chr($b2) . chr($b3);
            line($e, sprintf('8f%02x', $b2), $strs, $dump);
        }
    }
    if ($e === 'EUC-TW') {
        for ($b2 = 0xA0; $b2 <= 0xB1; $b2++) foreach (array_merge(range(0xA0, 0xFF), [0, 0x41, 0x80]) as $b3) {
            $strs = [];
            foreach ($all as $b4) $strs[] = "\x8E" . chr($b2) . chr($b3) . chr($b4);
            line($e, sprintf('8e%02x%02x', $b2, $b3), $strs, $dump);
        }
    }
    if (in_array($e, $gb4)) {
        foreach ([0x81, 0x82, 0x83, 0x84, 0x85, 0x8F, 0x90, 0x95, 0xA0, 0xE3, 0xE4, 0xFD, 0xFE] as $b1) for ($b2 = 0x30; $b2 <= 0x39; $b2++) {
            $strs = [];
            foreach ($all as $b3) foreach (array_merge(range(0x30, 0x39), [0, 0x2F, 0x3A, 0x81, 0xFF]) as $b4) $strs[] = chr($b1) . chr($b2) . chr($b3) . chr($b4);
            line($e, sprintf('%02x%02x', $b1, $b2), $strs, $dump);
        }
    }
    if (in_array($e, ['UTF-16', 'UTF-16BE', 'UTF-16LE', 'UTF-32', 'UTF-32BE', 'UTF-32LE', 'UCS-4', 'UCS-4BE', 'UCS-4LE', 'UCS-2', 'UCS-2BE', 'UCS-2LE'])) {
        foreach ([0x00, 0x41, 0xD7, 0xD8, 0xDB, 0xDC, 0xDF, 0xE0, 0xFE, 0xFF] as $b1) foreach ([0x00, 0x41, 0xD8, 0xDC, 0xFE, 0xFF] as $b2) foreach ([0x00, 0x10, 0x41, 0xD8, 0xDC, 0xDF, 0xFE, 0xFF] as $b3) {
            $strs = [];
            foreach ($all as $b4) $strs[] = chr($b1) . chr($b2) . chr($b3) . chr($b4);
            line($e, sprintf('%02x%02x%02x', $b1, $b2, $b3), $strs, $dump);
        }
    }
    if ($dump) fclose($dump);
}
