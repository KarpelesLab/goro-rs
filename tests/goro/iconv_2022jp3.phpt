--TEST--
iconv(): ISO-2022-JP-3
--FILE--
<?php
// ISO-2022-JP-3 (glibc): JIS X 0208 (ESC $ B), JIS X 0213 plane 1
// (ESC $ ( O, or ESC $ ( Q for the JIS X 0213:2004 additions), plane 2
// (ESC $ ( P), half-width katakana (ESC ( I); the set in use first, and
// か + U+309A as one plane 1 code
function e($cs, $s) { $r = @iconv("UTF-8", $cs, $s); return $r === false ? "false" : str_replace("\x1b", "^", $r); }
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
foreach (["\u{E6}\u{4E00}", "\u{4E00}\u{E6}\u{4E00}", "\u{4FF1}\u{4E00}\u{E6}", "\u{E6}\u{4FF1}\u{E6}", "\u{E6}\u{FF61}\u{E6}", "\u{E6}\u{A5}a",
          "\u{20089}\u{4E00}", "か\u{309A}か", "\u{E6}か\u{309A}", "かa", "か", "\u{2015}\u{2014}", "\u{CA}\u{304}", "\u{A5}\n\u{A5} \u{A5}"] as $s) {
    echo bin2hex($s), " ", e("ISO-2022-JP-3", $s), " ", e("ISO-2022-JP-3//IGNORE", $s . "\u{309A}"), "\n";
}
foreach (["1b24284f2477", "1b2428512e21", "1b24284f2e21", "1b2428502121", "1b2442213d", "1b24284f213d", "1b284921", "1b2849 60", "1b2441 3021", "1b242844 2237", "1b2e41"] as $h) {
    $h = str_replace(" ", "", $h);
    echo $h, " ", d("ISO-2022-JP-3", hex2bin($h)), "\n";
}
var_dump(iconv_strlen("\x1b\$(O\x24\x77\x1b(B", "ISO-2022-JP-3"));
--EXPECT--
c3a6e4b880 ^$(O)\0l^(B ^$(O)\0l^(B
e4b880c3a6e4b880 ^$B0l^$(O)\0l^(B ^$B0l^$(O)\0l^(B
e4bfb1e4b880c3a6 ^$(Q.!0l)\^(B ^$(Q.!0l)\^(B
c3a6e4bfb1c3a6 ^$(O)\^$(Q.!)\^(B ^$(O)\^$(Q.!)\^(B
c3a6efbda1c3a6 ^$(O)\^(I!^$(O)\^(B ^$(O)\^(I!^$(O)\^(B
c3a6c2a561 ^$(O)\^(J\a^(B ^$(O)\^(J\a^(B
f0a08289e4b880 ^$(P!!^$B0l^(B ^$(P!!^$B0l^(B
e3818be3829ae3818b ^$(O$w$+^(B ^$(O$w$w^(B
c3a6e3818be3829a ^$(O)\$w^(B ^$(O)\$w^(B
e3818b61 ^$B$+^(Ba ^$B$+^(Ba
e3818b ^$B$+^(B ^$(O$w^(B
e28095e28094 ^$B!=^$(O!=^(B ^$B!=^$(O!=^(B
c38acc84 ^$(O)A+[^(B ^$(O)A+[^(B
c2a50ac2a520c2a5 ^(J\^(B
^(J\^(B ^(J\^(B ^(J\^(B
^(J\^(B ^(J\^(B
1b24284f2477 e3818be3829a
1b2428512e21 e4bfb1
1b24284f2e21 e4bfb1
1b2428502121 f0a08289
1b2442213d e28095
1b24284f213d e28094
1b284921 efbda1
1b284960 false
1b24413021 1b24413021
1b2428442237 1b2428442237
1b2e41 1b2e41
int(2)
