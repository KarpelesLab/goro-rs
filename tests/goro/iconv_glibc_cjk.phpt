--TEST--
iconv(): glibc's CJK lead bytes and mapping differences
--FILE--
<?php
// glibc's CJK tables and lead byte handling
function d($cs, $s) {
    error_clear_last();
    $r = @iconv($cs, "UTF-8", $s);
    echo $cs, " ", bin2hex($s), ": ", $r === false ? "FALSE " . (error_get_last()['message'] ?? '') : bin2hex($r), "\n";
}
foreach (["SJIS" => ["\xeb", "\xea", "\x81"], "EUC-KR" => ["\xa0", "\xff", "\xa2\xe8"], "UHC" => ["\xc9", "\xfe"], "BIG5" => ["\x80", "\x81", "\xfa", "\xa1\x45", "\xa1\x4e", "\xc6\xa1", "\xa1\xc2"], "EUC-JP" => ["\x8f\xa1", "\x8f\xa2", "\xa0"], "GB2312" => ["\x81", "\x8e"], "CP950" => ["\xfa\x40"]] as $cs => $list) {
    foreach ($list as $s) d($cs, $s);
}
foreach (["EUC-KR" => "\u{20A9}\u{327E}", "BIG5" => "\u{AF}\u{2CD}\u{A2}\u{401}\u{F6B1}", "EUC-JP-MS" => "\u{A2}\u{A6}\u{2014}\u{80}", "CP950" => "\u{E000}"] as $cs => $s) {
    var_dump(bin2hex(@iconv("UTF-8", "$cs//IGNORE", $s)));
}
--EXPECT--
SJIS eb: FALSE iconv(): Detected an illegal character in input string
SJIS ea: FALSE iconv(): Detected an incomplete multibyte character in input string
SJIS 81: FALSE iconv(): Detected an incomplete multibyte character in input string
EUC-KR a0: FALSE iconv(): Detected an illegal character in input string
EUC-KR ff: FALSE iconv(): Detected an incomplete multibyte character in input string
EUC-KR a2e8: e389be
UHC c9: FALSE iconv(): Detected an illegal character in input string
UHC fe: FALSE iconv(): Detected an illegal character in input string
BIG5 80: c280
BIG5 81: FALSE iconv(): Detected an illegal character in input string
BIG5 fa: FALSE iconv(): Detected an illegal character in input string
BIG5 a145: e280a7
BIG5 a14e: efb991
BIG5 c6a1: ef9ab1
BIG5 a1c2: c2af
EUC-JP 8fa1: FALSE iconv(): Detected an illegal character in input string
EUC-JP 8fa2: FALSE iconv(): Detected an incomplete multibyte character in input string
EUC-JP a0: FALSE iconv(): Detected an incomplete multibyte character in input string
GB2312 81: FALSE iconv(): Detected an illegal character in input string
GB2312 8e: FALSE iconv(): Detected an incomplete multibyte character in input string
CP950 fa40: FALSE iconv(): Detected an illegal character in input string
string(8) "a3dca2e8"
string(12) "a1c2a1c5c6a1"
string(16) "a1f18fa2c3a1bd80"
string(0) ""
