--TEST--
iconv_mime_decode(_headers)(): failed raw text, separator-only charset names
--FILE--
<?php
// a header line whose only text failed to convert still counts as decoded
foreach ([0, 1, 2, 3] as $m) {
    var_dump(@iconv_mime_decode_headers("A: \n\u{20AC}\r\nB: =?UTF-7?Q?=0A+IKw-?=\r\n", $m));
    var_dump(@iconv_mime_decode_headers("A: x\r\n\xff\xfe\r\nB: y\r\n", $m));
}
// trailing separators of a charset name are dropped (",": the default charset)
foreach (["=?,?=?)\u{20AC}", "=?,?=?)", "=?UTF-8,?B?w6Q=?=", "=?/?B?w6Q=?=", "=?BAD?B?w6Q=?=\u{20AC}"] as $s) {
    foreach ([0, 2] as $m) {
        var_dump(@iconv_mime_decode($s, $m, "UTF-8"));
    }
}
var_dump(iconv("UTF-8,", "ISO-8859-1/", "\u{e9}") === "\xe9", iconv(",", "UTF-8", "\u{e9}"));
--EXPECT--
bool(false)
bool(false)
bool(false)
bool(false)
array(2) {
  ["A"]=>
  string(0) ""
  ["B"]=>
  string(4) "
€"
}
array(2) {
  ["A"]=>
  string(1) "x"
  ["B"]=>
  string(1) "y"
}
array(2) {
  ["A"]=>
  string(0) ""
  ["B"]=>
  string(4) "
€"
}
array(2) {
  ["A"]=>
  string(1) "x"
  ["B"]=>
  string(1) "y"
}
bool(false)
string(7) "=?,?=?)"
bool(false)
string(7) "=?,?=?)"
string(2) "ä"
string(2) "ä"
string(2) "ä"
string(2) "ä"
bool(false)
string(14) "=?BAD?B?w6Q=?="
bool(true)
string(2) "é"
