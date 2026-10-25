--TEST--
iconv(): only glibc's charset names and aliases, ICONV_VERSION
--FILE--
<?php
var_dump(ICONV_VERSION);
// glibc knows the names in its alias lists only (no loosened spellings)
foreach (["utf8", "Utf-8", "UTF_8", "latin1", "LATIN-1", "ISO8859-1", "iso_8859-1", "ISO_8859-1:1987", "ISO-8859-1:1987", "CP-1252", "WINDOWS1252", "UJIS", "OSF00030010", "CSEUCKR", "MSCP949", "CN-GB", "CSGB2312", "BIGFIVE", "CP950", "SJIS-WIN", "CSWINDOWS31J", "EUCJP-OPEN", "ISO2022JP", "CSISO2022KR", "UTF7", "CSUNICODE", "x"] as $n) {
    $r = @iconv($n, "UTF-8", "ab");
    echo json_encode($n), ": ", $r === false ? "F" : $r, "\n";
}
--EXPECT--
string(4) "2.43"
"utf8": ab
"Utf-8": ab
"UTF_8": F
"latin1": ab
"LATIN-1": F
"ISO8859-1": ab
"iso_8859-1": ab
"ISO_8859-1:1987": ab
"ISO-8859-1:1987": F
"CP-1252": F
"WINDOWS1252": F
"UJIS": ab
"OSF00030010": ab
"CSEUCKR": ab
"MSCP949": ab
"CN-GB": ab
"CSGB2312": ab
"BIGFIVE": ab
"CP950": ab
"SJIS-WIN": ab
"CSWINDOWS31J": ab
"EUCJP-OPEN": ab
"ISO2022JP": ab
"CSISO2022KR": ab
"UTF7": ab
"CSUNICODE": 扡
"x": F
