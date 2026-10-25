--TEST--
iconv() //TRANSLIT: glibc's C.UTF-8 transliteration tables
--FILE--
<?php
// glibc's C.UTF-8 transliteration tables (translit_combining, translit_neutral and its includes)
$s = "Aé€ｱ«½ﬁ™Ω ẛ µ ① ℌ ㎕ ﹏ ＡＢ ♥ 😀 ‰ ‐ Ǆ ŀ";
foreach (["ASCII", "ISO-8859-1", "GBK", "SJIS", "CP1251"] as $to) {
    echo $to, ": ", bin2hex(iconv("UTF-8", "$to//TRANSLIT", $s)), "\n";
}
echo iconv("UTF-8", "ASCII//TRANSLIT", "Ærøskøbing ½ → ≤ ≠ “quotes” ‘x’ — … •"), "\n";
var_dump(@iconv("UTF-8", "ASCII//TRANSLIT", "日本"));
// no private use mappings in glibc's GBK / SJIS
var_dump(bin2hex(iconv("UTF-8", "GBK//TRANSLIT", "\u{E000}")));
var_dump(bin2hex(iconv("UTF-8", "SJIS", "\u{FFE0}\u{FFE1}\u{FFE2}")));
var_dump(bin2hex(iconv("UTF-8", "CP932", "\u{A2}\u{A3}\u{AC}\u{2016}\u{2212}\u{301C}")));
--EXPECT--
ASCII: 41654555523f3c3c20312f3220666928544d293f203f207520283129204820756c205f204142203c33203a2d44203f202d203f206c
ISO-8859-1: 41e94555523fabbd666928544d293f203f20b520283129204820b56c205f204142203c33203a2d44203f202d203f206c
GBK: 41a8a680a5a23c3c20312f3220666928544d29a6b8203f20a6cc20a2d9204820a6cc6c20a96e20a3c1a3c2203c33203a2d4420a1eb20a95c203f206c
SJIS: 4165455552b13c3c20312f3220666928544d2983b6203f2083ca2028312920482083ca6c205f2082608261203c33203a2d442081f120815d203f206c
CP1251: 4165883fab20312f32206669993f203f20b520283129204820b56c205f204142203c33203a2d442089202d203f206c
AEroskobing  1/2  -> <= != "quotes" 'x' -- ... o
string(2) "??"
string(2) "3f"
string(12) "8191819281ca"
string(24) "8191819281ca8161817c8160"
