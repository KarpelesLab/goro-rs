--TEST--
iconv(): JOHAB
--FILE--
<?php
// JOHAB (glibc): KS X 1001 hangul by its jamo, the symbols and hanja
// rearranged; //IGNORE skips both bytes of a well-formed jamo combination
// that is no hangul, one byte otherwise
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
function di($cs, $s) { $r = @iconv($cs, "UTF-8//IGNORE", $s); return $r === false ? "false" : bin2hex($r); }
foreach (["JOHAB", "CP1361", "MSCP1361", "johab"] as $cs) {
    echo $cs, " ", bin2hex(iconv("UTF-8", $cs, "A가힣\u{3131}\u{4E00}\u{20A9}·★")), "\n";
}
foreach (["8861", "d3bd", "8441", "8444", "d931", "e031", "f9fe", "5c", "41"] as $h) {
    echo $h, " ", d("JOHAB", hex2bin($h)), "\n";
}
foreach (["8442", "8442", "8852", "d4a1", "8841", "88", "d9", "ff"] as $h) {
    echo $h, " ", d("JOHAB", hex2bin($h)), " ", di("JOHAB", hex2bin($h) . "8861"), "\n";
}
var_dump(@iconv("UTF-8", "JOHAB", "\\"), iconv("UTF-8", "JOHAB//TRANSLIT", "\u{1F600}x"));
var_dump(iconv_strlen(iconv("UTF-8", "JOHAB", "가나다라"), "JOHAB"));
--EXPECT--
JOHAB 418861d3bd8841f1795cd934d96a
CP1361 418861d3bd8841f1795cd934d96a
MSCP1361 418861d3bd8841f1795cd934d96a
johab 418861d3bd8841f1795cd934d96a
8861 eab080
d3bd ed9ea3
8441 false
8444 e384b3
d931 e38080
e031 e4bcbd
f9fe e8a9b0
5c e282a9
41 41
8442 false 38383631
8442 false 38383631
8852 false 5238383631
d4a1 false 38383631
8841 e384b1 e384b138383631
88 false 38383631
d9 false e38083383631
ff false 38383631
bool(false)
string(4) ":-Dx"
int(4)
