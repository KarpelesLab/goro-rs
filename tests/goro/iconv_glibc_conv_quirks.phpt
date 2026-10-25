--TEST--
iconv(): tag characters, //IGNORE before an incomplete character, //TRANSLIT shift states, ISO-2022-JP Roman
--FILE--
<?php
// glibc conversion details: encoders drop the Unicode tag characters they
// cannot write (not ISO-2022-JP); //IGNORE reports a skipped character
// before an incomplete one; //TRANSLIT converts each alternative in a
// nested call (a failed one keeps its shift state, ISO-2022-KR writes its
// header again during the first call); ISO-2022-JP stays in JIS X 0201
// Roman for graphic characters only and //IGNORE skips one byte of an
// unassigned JIS X 0208 pair
function e($cs, $s) { $r = @iconv("UTF-8", $cs, $s); return $r === false ? "false" : bin2hex($r); }
foreach (["EUC-JP", "ISO-8859-1", "ASCII", "SJIS", "ISO-2022-JP", "ISO-2022-JP-3", "BIG5", "UCS-2", "ISO-2022-KR", "CP1252", "JOHAB"] as $cs) {
    echo $cs, " ", e($cs, "a\u{E0001}\u{E006B}\u{E007F}b"), "\n";
}
foreach (["€ab\xe4", "€ab\xe4\xb8", "ab\xe4", "\xffab\xe4", "€\xffab\xe4"] as $s) {
    $r = @iconv("UTF-8", "ISO-8859-1//IGNORE", $s);
    echo bin2hex($s), " ", $r === false ? "false " . error_get_last()['message'] : bin2hex($r), "\n";
}
foreach (["Ê", "aÊ", "中Ê", "\u{10FFFF}", "a\u{10FFFF}", "日゛", "日゛日", "中\u{10FFFF}中"] as $s) {
    echo bin2hex($s), " ", e("ISO-2022-KR//TRANSLIT", $s), " ", e("ISO-2022-CN//TRANSLIT", $s), "\n";
}
foreach (["\u{A5}\n\u{A5}", "\u{A5} \u{A5}", "\u{A5}a\u{A5}", "\u{A5}~\u{A5}", "\u{A5}\x7f"] as $s) {
    echo bin2hex($s), " ", e("ISO-2022-JP", $s), " ", e("ISO-2022-JP-2", $s), "\n";
}
foreach (["1b2442582128414142", "1b244258211b284142"] as $h) {
    $r = @iconv("ISO-2022-JP", "UTF-8//IGNORE", hex2bin($h));
    echo $h, " ", $r === false ? "false" : bin2hex($r), "\n";
}
var_dump(iconv_strpos("\x1b\$)C\x0e\x30\x21\x0f", "\x0f", 0, "ISO-2022-KR"));
--EXPECT--
EUC-JP 6162
ISO-8859-1 6162
ASCII 6162
SJIS 6162
ISO-2022-JP false
ISO-2022-JP-3 6162
BIG5 6162
UCS-2 61006200
ISO-2022-KR 1b2429436162
CP1252 6162
JOHAB 6162
e282ac6162e4 6162
e282ac6162e4b8 6162
6162e4 false iconv(): Detected an incomplete multibyte character in input string
ff6162e4 false iconv(): Detected an incomplete multibyte character in input string
e282acff6162e4 6162
c38a 1b2429431b24294345 45
61c38a 1b242943611b24294345 6145
e4b8adc38a 1b2429430e71691b2429430f45 1b2429410e56500f450f
f48fbfbf 1b2429431b2429433f 3f
61f48fbfbf 1b242943611b2429433f 613f
e697a5e3829b 1b2429430e6c6d1b2429433f 1b2429410e48553f0f
e697a5e3829be697a5 1b2429430e6c6d1b2429433f0e6c6d0f 1b2429410e48553f0e48550f
e4b8adf48fbfbfe4b8ad 1b2429430e71691b2429430f3f0e71690f 1b2429410e56500f3f0e56500f
c2a50ac2a5 1b284a5c1b28420a1b284a5c1b2842 1b284a5c1b28420a1b284a5c1b2842
c2a520c2a5 1b284a5c1b2842201b284a5c1b2842 1b284a5c1b2842201b284a5c1b2842
c2a561c2a5 1b284a5c615c1b2842 1b284a5c615c1b2842
c2a57ec2a5 1b284a5c1b28427e1b284a5c1b2842 1b284a5c1b28427e1b284a5c1b2842
c2a57f 1b284a5c1b28427f 1b284a5c1b28427f
1b2442582128414142 false
1b244258211b284142 e682841be7968e
bool(false)
