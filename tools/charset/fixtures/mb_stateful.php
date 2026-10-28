<?php
// Decode fixture for the stateful encodings: for each (encoding, prefix),
// every 1- and 2-byte suffix is appended to the prefix and converted to
// UTF-32BE with substitute character '%'. Records as in mb_decode.php.
// Then an encode fixture: every ordered pair of a set of sample code points.
// Usage: php mb_stateful.php [DUMPDIR]
error_reporting(E_ALL & ~E_DEPRECATED);
mb_substitute_character(0x25);
$dumpdir = $argv[1] ?? null;

$jis = ["", "\x1B\$B", "\x1B\$@", "\x1B\$(D", "\x1B(I", "\x1B(J", "\x0E", "\x1B", "\x1B\$", "\x1B\$(", "\x1B(", "\x1B\$B\x21", "\x1B\$(Q", "\x1B\$(P", "\x1B\$(O", "\x1B\$B\x1B(B"];
$probes = [
    'JIS' => $jis, 'ISO-2022-JP' => $jis, 'ISO-2022-JP-MS' => $jis, 'CP50220' => $jis,
    'CP50221' => $jis, 'CP50222' => $jis, 'ISO-2022-JP-2004' => $jis,
    'ISO-2022-JP-MOBILE#KDDI' => $jis,
    'UTF-7' => ["", "+", "+A", "+AA", "+AAA", "+AAAA", "+AAAAA", "+AGE", "+2D3", "+2D3e", "+2D3eA", "+2D3eAA", "+AGEA", "a+AGE-"],
    'UTF7-IMAP' => ["", "&", "&A", "&AA", "&AAA", "&AAAA", "&AAAAA", "&AGE", "&2D3", "&2D3e", "&2D3eA", "&AGEA", "a&AGE-"],
    'HZ' => ["", "~{", "~{\x21", "~{\x30\x21", "~"],
    'ISO-2022-KR' => ["", "\x1B\$)C", "\x1B\$)C\x0E", "\x0E", "\x1B\$)C\x0E\x30"],
    'SJIS-Mobile#SOFTBANK' => ["\x1B\$G", "\x1B\$E", "\x1B\$F", "\x1B\$O", "\x1B\$P", "\x1B\$Q", "\x1B\$"],
];

function rec($s, $e) {
    $ill = mb_get_info('illegal_chars');
    $out = mb_convert_encoding($s, 'UTF-32BE', $e);
    $d = mb_get_info('illegal_chars') - $ill;
    return (mb_check_encoding($s, $e) ? '1' : '0') . mb_strlen($s, $e) . ':' . $d . ':' . bin2hex($out) . ',';
}
foreach ($probes as $e => $prefixes) {
    $dump = $dumpdir ? fopen("$dumpdir/" . str_replace('#', '_', $e) . ".st", 'w') : null;
    foreach ($prefixes as $p) {
        $ph = bin2hex($p) ?: '-';
        $r = '';
        for ($b = 0; $b < 256; $b++) $r .= rec($p . chr($b), $e);
        printf("%s %s/1 %08x\n", $e, $ph, crc32($r));
        if ($dump) fwrite($dump, "$ph/1 $r\n");
        for ($b1 = 0; $b1 < 256; $b1++) {
            $r = '';
            for ($b2 = 0; $b2 < 256; $b2++) $r .= rec($p . chr($b1) . chr($b2), $e);
            printf("%s %s/%02x %08x\n", $e, $ph, $b1, crc32($r));
            if ($dump) fwrite($dump, sprintf("%s/%02x %s\n", $ph, $b1, $r));
        }
    }
    if ($dump) fclose($dump);
}

// Encode: all ordered pairs of sample code points, per encoding.
$cps = [0x00, 0x0A, 0x0E, 0x0F, 0x1B, 0x20, 0x41, 0x5C, 0x7E, 0x7F, 0x80, 0xA5, 0xA6, 0xA7, 0xAF,
    0x203E, 0x2015, 0x2016, 0x2116, 0x2160, 0x2170, 0x2225, 0x3000, 0x3042, 0x304B, 0x309A,
    0x30AB, 0x31F7, 0x4E00, 0x4E02, 0x4E28, 0x7E8A, 0x9ED1, 0xE000, 0xE3AC, 0xE757, 0xE63E,
    0xEE70, 0xF8F0, 0xFF01, 0xFF5E, 0xFF61, 0xFF71, 0xFF9F, 0xFFE2, 0xFFE4, 0x1F600, 0x20089,
    0x2B, 0x2D, 0x26, 0x2F, 0x3D, 0x7A, 0xD800, 0x10FFFF, 0x00E6, 0x0300, 0x02E9, 0x02E5, 0xD55C, 0x4E2D];
$all = mb_list_encodings();
foreach ($all as $e) {
    $dump = $dumpdir ? fopen("$dumpdir/" . str_replace('#', '_', $e) . ".pairs", 'w') : null;
    foreach ($cps as $a) {
        $r = '';
        foreach ($cps as $b) {
            $ill = mb_get_info('illegal_chars');
            $out = mb_convert_encoding(pack('NN', $a, $b), $e, 'UTF-32BE');
            $r .= (mb_get_info('illegal_chars') - $ill) . ':' . bin2hex($out) . ',';
        }
        printf("%s pair%x %08x\n", $e, $a, crc32($r));
        if ($dump) fwrite($dump, sprintf("pair%x %s\n", $a, $r));
    }
    if ($dump) fclose($dump);
}
