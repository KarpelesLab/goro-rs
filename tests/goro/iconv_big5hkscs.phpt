--TEST--
iconv(): BIG5-HKSCS and its combining sequences
--FILE--
<?php
// BIG5-HKSCS (glibc): four sequences decode to a letter and a combining
// mark, and the encoder holds back Ê / ê until it sees whether the next
// character combines with them
function e($cs, $s) { $r = @iconv("UTF-8", $cs, $s); return $r === false ? "false" : bin2hex($r); }
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
foreach (["BIG5-HKSCS", "BIG5HKSCS"] as $cs) {
    echo $cs, " ", e($cs, "一\u{CA}\u{304}\u{CA}\u{30C}\u{CA}\u{EA}\u{304}x\u{EA}"), "\n";
}
foreach (["8862", "8864", "88a3", "88a5", "8866", "a440", "c6a1", "8740", "fefe", "80", "a3e1"] as $h) {
    echo $h, " ", d("BIG5-HKSCS", hex2bin($h)), "\n";
}
foreach (["\u{CA}\u{10FFFF}\u{304}", "\u{CA}\u{E0001}\u{304}", "\u{304}", "\u{E6}", "\u{E6}\u{300}"] as $s) {
    echo bin2hex($s), " ", e("BIG5-HKSCS", $s), " ", e("BIG5-HKSCS//IGNORE", $s), " ", e("BIG5-HKSCS//TRANSLIT", $s), "\n";
}
var_dump(iconv_strlen("\x88\x62\x88\x66", "BIG5-HKSCS"), bin2hex(iconv_substr("a\x88\x62b", 1, 1, "BIG5-HKSCS")));
var_dump(bin2hex(iconv("BIG5-HKSCS", "BIG5-HKSCS", "\x88\x62\x88\x64\x88\x66")));
--EXPECT--
BIG5-HKSCS a44088628864886688a37888a7
BIG5HKSCS a44088628864886688a37888a7
8862 c38acc84
8864 c38acc8c
88a3 c3aacc84
88a5 c3aacc8c
8866 c38a
a440 e4b880
c6a1 e291a0
8740 e48fb0
fefe e7a794
80 c280
a3e1 false
c38af48fbfbfcc84 false 8866 88663f
c38af3a08081cc84 false 8866 8866
cc84 false  
c3a6 false  6165
c3a6cc80 false  6165
int(3)
string(4) "8862"
string(12) "886288648866"
