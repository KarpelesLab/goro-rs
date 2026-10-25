--TEST--
iconv(): GB18030 (glibc's GB18030-2022) and EUC-TW
--FILE--
<?php
function d($cs, $s) {
    foreach (["", "//IGNORE"] as $m) {
        error_clear_last();
        $r = @iconv($cs, "UTF-8$m", $s);
        echo $cs, $m, " ", bin2hex($s), ": ", $r === false ? "FALSE" : bin2hex($r), " ", error_get_last()['message'] ?? '', "\n";
    }
}
// GB18030-2022 (glibc): two- and four-byte sequences
foreach (["\x81\x30\x81\x30", "\x90\x30\x81\x30", "\xa8\xbc", "\xfe\x59", "\x81\x00", "\x82\x39\xff\x61", "\x90\x31\xa2\x8e", "\xb0\x7f\x8e\x81", "\x90\x3a\xb0\x8e", "\x81", "\x81\x30\x81", "\x80", "\xff"] as $s) d("GB18030", $s);
foreach (["\u{80}", "\u{1E3F}", "\u{9FB4}", "\u{E78D}", "\u{FE10}", "\u{10000}", "\u{10FFFF}", "日本語€"] as $c) {
    var_dump(bin2hex(iconv("UTF-8", "GB18030", $c)));
}
var_dump(iconv_mime_encode("Subject", "中文主题", ["output-charset" => "GB18030", "scheme" => "Q"]));
var_dump(iconv_mime_decode("=?GB18030?B?1tDOxNb3zOI=?=", 0, "UTF-8"));
// EUC-TW: CNS 11643 planes 1 and 2
foreach (["\xc4\xa1", "\x8e\xa2\xa1\xa1", "\x8e\xa1\xa4\xa1", "\x8e\x00", "\x8e\xa1", "\xa7\xa8"] as $s) d("EUC-TW", $s);
var_dump(bin2hex(iconv("UTF-8", "EUC-TW", "中文\u{4EA0}")));
--EXPECT--
GB18030 81308130: c280 
GB18030//IGNORE 81308130: c280 
GB18030 90308130: f0908080 
GB18030//IGNORE 90308130: f0908080 
GB18030 a8bc: e1b8bf 
GB18030//IGNORE a8bc: e1b8bf 
GB18030 fe59: e9beb4 
GB18030//IGNORE fe59: e9beb4 
GB18030 8100: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE 8100:  
GB18030 8239ff61: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE 8239ff61: 61 
GB18030 9031a28e: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE 9031a28e:  
GB18030 b07f8e81: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE b07f8e81: e5b7b5 
GB18030 903ab08e: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE 903ab08e: e79aab 
GB18030 81: FALSE iconv(): Detected an incomplete multibyte character in input string
GB18030//IGNORE 81: FALSE iconv(): Detected an incomplete multibyte character in input string
GB18030 813081: FALSE iconv(): Detected an incomplete multibyte character in input string
GB18030//IGNORE 813081: FALSE iconv(): Detected an incomplete multibyte character in input string
GB18030 80: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE 80:  
GB18030 ff: FALSE iconv(): Detected an illegal character in input string
GB18030//IGNORE ff:  
string(8) "81308130"
string(4) "a8bc"
string(4) "fe59"
string(8) "84318236"
string(4) "a6d9"
string(8) "90308130"
string(8) "e3329a35"
string(16) "c8d5b1bed55aa2e3"
string(47) "Subject: =?GB18030?Q?=D6=D0=CE=C4=D6=F7=CC=E2?="
string(12) "中文主题"
EUC-TW c4a1: e4b880 
EUC-TW//IGNORE c4a1: e4b880 
EUC-TW 8ea2a1a1: e4b982 
EUC-TW//IGNORE 8ea2a1a1: e4b982 
EUC-TW 8ea1a4a1: efbc90 
EUC-TW//IGNORE 8ea1a4a1: efbc90 
EUC-TW 8e00: FALSE iconv(): Detected an illegal character in input string
EUC-TW//IGNORE 8e00: 00 
EUC-TW 8ea1: FALSE iconv(): Detected an incomplete multibyte character in input string
EUC-TW//IGNORE 8ea1: FALSE iconv(): Detected an incomplete multibyte character in input string
EUC-TW a7a8: e4baa0 
EUC-TW//IGNORE a7a8: e4baa0 
string(12) "c4e3c5c6a7a8"
