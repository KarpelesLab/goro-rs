--TEST--
iconv(): CP1255, CP1258 and TCVN compose and decompose combining characters
--FILE--
<?php
// CP1255 / CP1258 / TCVN5712-1 compose base + combining characters when
// decoding and decompose when encoding (glibc)
foreach (["CP1258" => ["A\xcc", "a\xec\xd2", "\xd5\xf2", "\xcc", "Vi\xd2\xeat"], "WINDOWS-1255" => ["\xf9\xd1", "\xf9\xcc\xd1", "\xe9\xc4", "\xc4"], "TCVN" => ["a\xb3", "A\xb0", "\xb3"]] as $cs => $list) {
    foreach ($list as $s) {
        echo $cs, " ", bin2hex($s), " ", bin2hex(iconv($cs, "UTF-8", $s)), "\n";
    }
}
foreach (["CP1258" => "Tiếng Việt ẢẠ\u{340}", "CP1255" => "\u{FB2C}\u{FB1D}\u{FB49}שׁ", "TCVN5712-1" => "Tiếng Việt"] as $cs => $s) {
    echo $cs, " ", bin2hex(iconv("UTF-8", $cs, $s)), "\n";
}
var_dump(iconv_strlen("Vi\xd2\xeat", "CP1258"), iconv_strpos("A\xccB", "B", 0, "CP1258"));
--EXPECT--
CP1258 41cc c380
CP1258 61ecd2 c3a1cc89
CP1258 d5f2 e1bba2
CP1258 cc cc80
CP1258 5669d2ea74 56e1bb89c3aa74
WINDOWS-1255 f9d1 efacaa
WINDOWS-1255 f9ccd1 efacac
WINDOWS-1255 e9c4 efac9d
WINDOWS-1255 c4 d6b4
TCVN 61b3 c3a1
TCVN 41b0 c380
TCVN b3 cc81
CP1258 5469eaec6e67205669eaf2742041d241f2cc
CP1255 f9ccd1e9c4f9ccf9d1
TCVN5712-1 5469d56e67205669d674
int(4)
int(1)
