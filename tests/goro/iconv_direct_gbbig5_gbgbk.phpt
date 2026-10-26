--TEST--
iconv(): glibc's direct GB2312 <-> BIG5 and GBK <-> GB2312 modules
--FILE--
<?php
// glibc converts EUC-CN <-> BIG5 (GBBIG5) and GBK <-> EUC-CN (GBGBK)
// directly, not through UCS-4: GB2312 -> BIG5 maps simplified characters
// to their traditional forms, //IGNORE writes a box (BIG5 A1BC, GB2312
// A1F5) for a character without a mapping; GBK -> GB2312 rejects the
// pairs GB2312 lacks with both bytes; GB2312 -> GBK wants a byte after
// a two-byte character (one ending the input is "incomplete")
function h($r) { return $r === false ? "false" : bin2hex($r); }
foreach (["\xd6\xd0\xb9\xfa", "\xbc\xf2\xcc\xe5\xd7\xd6", "\xba\xba\xd3\xef", "a\xa5\xe6b", "\xac\xef", "\xa1\xa4\xa1\xaa", "\xe6\x97\xa5", "\xb0\xff"] as $s) {
    echo "GB2312->BIG5 ", bin2hex($s), " ", h(@iconv("GB2312", "BIG5", $s)), " ", h(@iconv("GB2312", "BIG5//IGNORE", $s)), " ", h(@iconv("EUC-CN", "BIG5//TRANSLIT", $s)), "\n";
}
foreach (["\xa4\xa4\xb0\xea", "\xc5\xe9\xa6\x72", "\xa1\x56x", "\xa1", "\xfa\x40"] as $s) {
    echo "BIG5->GB2312 ", bin2hex($s), " ", h(@iconv("BIG5", "GB2312", $s)), " ", h(@iconv("BIG5", "GB2312//IGNORE", $s)), "\n";
}
foreach (["\xb0\xa1", "\xef\x24\xc3\xa4B", "\x81\x40x", "\xa2\xa5x", "\xa6\xe0x", "\xb0"] as $s) {
    echo "GBK->GB2312 ", bin2hex($s), " ", h(@iconv("GBK", "GB2312", $s)), " ", h(@iconv("CP936", "EUC-CN//IGNORE", $s)), "\n";
}
foreach (["\xb0\xa1", "\xb0\xa1x", "x\xb0\xa1", "\x81\x40x", "abc"] as $s) {
    echo "GB2312->GBK ", bin2hex($s), " ", h(@iconv("GB2312", "GBK", $s)), "\n";
}
var_dump(iconv("GB2312", "GBK", "\xb0\xa1"));
var_dump(iconv("GB2312", "BIG5", "\xa5\xe6"));
// through UCS-4 the same characters convert as usual
echo bin2hex(iconv("UTF-8", "BIG5", iconv("GB2312", "UTF-8", "\xd6\xd0\xb9\xfa"))), "\n";
?>
--EXPECTF--
GB2312->BIG5 d6d0b9fa a4a4b0ea a4a4b0ea a4a4b0ea
GB2312->BIG5 bcf2cce5d7d6 c2b2c5e9a672 c2b2c5e9a672 c2b2c5e9a672
GB2312->BIG5 babad3ef ba7ebb79 ba7ebb79 ba7ebb79
GB2312->BIG5 61a5e662 false 61a1bc62 false
GB2312->BIG5 acef false a1bc false
GB2312->BIG5 a1a4a1aa a150a277 a150a277 a150a277
GB2312->BIG5 e697a5 false false false
GB2312->BIG5 b0ff c1a1 c1a1 c1a1
BIG5->GB2312 a4a4b0ea d6d0b9fa d6d0b9fa
BIG5->GB2312 c5e9a672 cce5d7d6 cce5d7d6
BIG5->GB2312 a15678 false a1f578
BIG5->GB2312 a1 false false
BIG5->GB2312 fa40 false 40
GBK->GB2312 b0a1 b0a1 b0a1
GBK->GB2312 ef24c3a442 false c3a442
GBK->GB2312 814078 false 78
GBK->GB2312 a2a578 false 78
GBK->GB2312 a6e078 false 78
GBK->GB2312 b0 false false
GB2312->GBK b0a1 false
GB2312->GBK b0a178 b0a178
GB2312->GBK 78b0a1 false
GB2312->GBK 814078 814078
GB2312->GBK 616263 616263

Notice: iconv(): Detected an incomplete multibyte character in input string in %s on line %d
bool(false)

Notice: iconv(): Detected an illegal character in input string in %s on line %d
bool(false)

Notice: iconv(): Detected an illegal character in input string in %s on line %d

