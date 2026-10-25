--TEST--
mb_convert_case(): input decoding to no characters still runs the encoder
--FILE--
<?php
// input decoding to no characters still reaches the encoder
var_dump(@mb_strtoupper("abc", "UUENCODE"));
var_dump(@mb_convert_case("\r\n\x80", MB_CASE_TITLE, "UUENCODE"));
var_dump(@mb_ucfirst("\r\n", "UUENCODE"));
var_dump(@mb_strtolower("", "UUENCODE"));
var_dump(bin2hex(mb_strtoupper("\x1b\$B", "ISO-2022-JP")));
var_dump(bin2hex(mb_strtoupper("+", "UTF-7")));
--EXPECT--
string(21) "begin 0644 filename
 "
string(21) "begin 0644 filename
 "
string(22) "begin 0644 filename
 
"
string(0) ""
string(0) ""
string(0) ""
