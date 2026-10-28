<?php
// Observation driver for ISO-2022-JP and ISO-2022-KR on the reference PHP 8.5
// (glibc 2.43 iconv):  php85 iconv_obs_iso2022.php > iso2022.out
// "J <escape-hex> <lead> <entries...>": decoding of every byte pair after an
//   escape sequence (entries as in iconv_obs_tables.php, "x" when illegal).
// "JE <cp> <bytes>": encoding of each BMP code point that encodes (outside ASCII).
// "T <label> <hex|x|i>": assorted sequences.
$GLOBALS['lasterr'] = '';
set_error_handler(function ($no, $msg) { $GLOBALS['lasterr'] = $msg; return true; });
function k($f, $t, $s) {
    $GLOBALS['lasterr'] = '';
    $r = iconv($f, $t, $s);
    if ($r !== false) return $r === '' ? 'e' : bin2hex($r);
    return strpos($GLOBALS['lasterr'], 'incomplete') ? 'i' : 'x';
}
function cps($h) {
    if ($h === 'x' || $h === 'i' || $h === 'e') return $h;
    return implode('+', array_map(fn($c) => sprintf('%X', $c), unpack('N*', hex2bin($h))));
}
foreach (['ISO-2022-JP' => ["\x1b(B", "\x1b(J", "\x1b\$@", "\x1b\$B", "\x1b(I", "\x1b\$(D", "\x1b\$A", "\x1b\$(C", "\x1b.A", "\x1b.F"],
          'ISO-2022-KR' => ["\x1b\$)C\x0e"]] as $cs => $escs) {
    foreach ($escs as $esc) {
        // Single bytes after the escape.
        $line = [];
        for ($b = 0; $b < 256; $b++) $line[] = cps(k($cs, 'UTF-32BE', $esc . chr($b)));
        echo "J $cs ", bin2hex($esc), " - ", implode(' ', $line), "\n";
        for ($l = 0x21; $l < 0x7F; $l++) {
            $line = [];
            for ($t = 0x21; $t < 0x7F; $t++) $line[] = cps(k($cs, 'UTF-32BE', $esc . chr($l) . chr($t)));
            echo "J $cs ", bin2hex($esc), " ", sprintf('%02x', $l), " ", implode(' ', $line), "\n";
        }
    }
    for ($cp = 0x80; $cp < 0x10000; $cp++) {
        if ($cp >= 0xD800 && $cp < 0xE000) continue;
        $r = k('UTF-32BE', $cs, pack('N', $cp));
        if ($r !== 'x') printf("JE %s %X %s\n", $cs, $cp, $r);
    }
    for ($cp = 0; $cp < 0x80; $cp++) {
        $r = k('UTF-32BE', $cs, pack('N', $cp));
        printf("JA %s %X %s\n", $cs, $cp, $r);
    }
}
$tests = [
    'jp a-kana-a' => ['UTF-8', 'ISO-2022-JP', "a\xe3\x81\x82a"],
    'jp yen' => ['UTF-8', 'ISO-2022-JP', "\xc2\xa5"],
    'jp yen a' => ['UTF-8', 'ISO-2022-JP', "\xc2\xa5a\\"],
    'jp overline' => ['UTF-8', 'ISO-2022-JP', "\xe2\x80\xbe"],
    'jp halfwidth' => ['UTF-8', 'ISO-2022-JP', "\xef\xbd\xb1"],
    'jp halfwidth translit' => ['UTF-8', 'ISO-2022-JP//TRANSLIT', "\xef\xbd\xb1"],
    'jp kana yen kana' => ['UTF-8', 'ISO-2022-JP', "\xe3\x81\x82\xc2\xa5\xe3\x81\x82"],
    'jp newline in kanji' => ['UTF-8', 'ISO-2022-JP', "\xe3\x81\x82\n\xe3\x81\x82"],
    'jp dec newline in kanji' => ['ISO-2022-JP', 'UTF-8', "\x1b\$B\$\"\n\$\"\x1b(B"],
    'jp dec unterminated' => ['ISO-2022-JP', 'UTF-8', "\x1b\$B\$\""],
    'jp dec half' => ['ISO-2022-JP', 'UTF-8', "\x1b\$B\$"],
    'jp dec esc only' => ['ISO-2022-JP', 'UTF-8', "\x1b"],
    'jp dec esc (' => ['ISO-2022-JP', 'UTF-8', "\x1b("],
    'jp dec esc $' => ['ISO-2022-JP', 'UTF-8', "\x1b\$"],
    'jp dec bad esc' => ['ISO-2022-JP', 'UTF-8', "\x1b(Z"],
    'jp dec SO' => ['ISO-2022-JP', 'UTF-8', "a\x0eb"],
    'jp dec 8bit' => ['ISO-2022-JP', 'UTF-8', "a\xa4\xa2b"],
    'jp dec ignore 8bit' => ['ISO-2022-JP', 'UTF-8//IGNORE', "a\xa4\xa2b"],
    'jp dec ignore bad pair' => ['ISO-2022-JP', 'UTF-8//IGNORE', "\x1b\$B\x22\x7e\$\"\x1b(B"],
    'jp dec roman' => ['ISO-2022-JP', 'UTF-8', "\x1b(J\\~a\x1b(B"],
    'jp enc after roman' => ['UTF-8', 'ISO-2022-JP', "\xc2\xa5abc\\"],
    'jp enc empty' => ['UTF-8', 'ISO-2022-JP', ""],
    'kr a' => ['UTF-8', 'ISO-2022-KR', "a"],
    'kr empty' => ['UTF-8', 'ISO-2022-KR', ""],
    'kr hangul' => ['UTF-8', 'ISO-2022-KR', "a\xea\xb0\x80b"],
    'kr hangul end' => ['UTF-8', 'ISO-2022-KR', "\xea\xb0\x80"],
    'kr hangul nl' => ['UTF-8', 'ISO-2022-KR', "\xea\xb0\x80\n\xea\xb0\x80"],
    'kr dec no header' => ['ISO-2022-KR', 'UTF-8', "\x0e\x30\x21\x0fa"],
    'kr dec header' => ['ISO-2022-KR', 'UTF-8', "\x1b\$)C\x0e\x30\x21\x0fa"],
    'kr dec header mid' => ['ISO-2022-KR', 'UTF-8', "a\x1b\$)C\x0e\x30\x21\x0fa"],
    'kr dec unterminated' => ['ISO-2022-KR', 'UTF-8', "\x1b\$)C\x0e\x30\x21"],
    'kr dec nl in so' => ['ISO-2022-KR', 'UTF-8', "\x1b\$)C\x0e\x30\x21\n\x30\x21"],
    'kr dec plain' => ['ISO-2022-KR', 'UTF-8', "abc"],
];
foreach ($tests as $label => [$f, $t, $s]) echo "T $label: ", k($f, $t, $s), "\n";
