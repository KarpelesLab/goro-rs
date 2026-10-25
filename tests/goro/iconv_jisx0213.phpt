--TEST--
iconv(): EUC-JISX0213 and SHIFT_JISX0213
--FILE--
<?php
// EUC-JISX0213 / SHIFT_JISX0213 (glibc): JIS X 0213 planes 1 and 2, with
// か + U+309A, æ + U+0300... as one code (decoded to two characters)
function e($cs, $s) { $r = @iconv("UTF-8", $cs, $s); return $r === false ? "false" : bin2hex($r); }
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
function di($cs, $s) { $r = @iconv($cs, "UTF-8//IGNORE", $s); return $r === false ? "false" : bin2hex($r); }
$s = "Aか\u{309A}か゚\u{E6}\u{300}\u{E6}\u{2E9}\u{2E5}\u{2E5}\u{2E9}\u{31F7}\u{309A}\u{20089}\u{4FF1}ｱ\\~\u{2014}";
foreach (["EUC-JISX0213", "SHIFT_JISX0213", "ShiftJISX0213", "Shift_JISX0213"] as $cs) {
    $b = iconv("UTF-8", $cs, $s);
    echo $cs, " ", bin2hex($b), " ", d($cs, $b), "\n";
}
foreach (["a4f7", "abc4", "a4ab", "8fa1a1", "8ea1", "8fa1", "8ffefe", "a1", "a1a0", "8e", "8ee0"] as $h) {
    echo "EUC ", $h, " ", d("EUC-JISX0213", hex2bin($h)), " ", di("EUC-JISX0213", hex2bin($h) . "41"), "\n";
}
foreach (["82f5", "8663", "f040", "fcfc", "5c", "7e", "a1", "80", "fd", "81"] as $h) {
    echo "SJIS ", $h, " ", d("SHIFT_JISX0213", hex2bin($h)), "\n";
}
foreach (["か\u{309A}\u{309A}", "x\u{309A}", "か\u{10FFFF}\u{309A}", "\u{E6}\u{10FFFF}\u{300}"] as $s) {
    echo bin2hex($s), " ", e("EUC-JISX0213", $s), " ", e("EUC-JISX0213//IGNORE", $s), " ", e("SHIFT_JISX0213//TRANSLIT", $s), "\n";
}
var_dump(iconv_strlen("\xa4\xf7\xa4\xab", "EUC-JISX0213"), iconv_strlen("\x82\xf5", "SHIFT_JISX0213"));
--EXPECT--
EUC-JISX0213 41a4f7a4f7abc4a9dcabe5abe6a6f88fa1a1aea18eb15c7ea1bd 41e3818be3829ae3818be3829ac3a6cc80c3a6cba9cba5cba5cba9e387b7e3829af0a08289e4bfb1efbdb15c7ee28094
SHIFT_JISX0213 4182f582f58663857b8685868683f6f040879fb15c7e815c 41e3818be3829ae3818be3829ac3a6cc80c3a6cba9cba5cba5cba9e387b7e3829af0a08289e4bfb1efbdb1c2a5e280bee28094
ShiftJISX0213 4182f582f58663857b8685868683f6f040879fb15c7e815c 41e3818be3829ae3818be3829ac3a6cc80c3a6cba9cba5cba5cba9e387b7e3829af0a08289e4bfb1efbdb1c2a5e280bee28094
Shift_JISX0213 4182f582f58663857b8685868683f6f040879fb15c7e815c 41e3818be3829ae3818be3829ac3a6cc80c3a6cba9cba5cba5cba9e387b7e3829af0a08289e4bfb1efbdb1c2a5e280bee28094
EUC a4f7 e3818be3829a e3818be3829a3431
EUC abc4 c3a6cc80 c3a6cc803431
EUC a4ab e3818b e3818b3431
EUC 8fa1a1 f0a08289 f0a082893431
EUC 8ea1 efbda1 efbda13431
EUC 8fa1 false 3431
EUC 8ffefe false e7b9ab3431
EUC a1 false 3431
EUC a1a0 false 3431
EUC 8e false 3431
EUC 8ee0 false 3431
SJIS 82f5 e3818be3829a
SJIS 8663 c3a6cc80
SJIS f040 f0a08289
SJIS fcfc false
SJIS 5c c2a5
SJIS 7e e280be
SJIS a1 efbda1
SJIS 80 false
SJIS fd false
SJIS 81 false
e3818be3829ae3829a false a4f7 82f5
78e3829a false 78 78
e3818bf48fbfbfe3829a false a4ab 82a93f
c3a6f48fbfbfcc80 false a9dcabdc 857b3f867b
int(3)
int(2)
