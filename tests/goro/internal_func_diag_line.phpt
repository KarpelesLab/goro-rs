--TEST--
pow()/utf8_encode()/utf8_decode() deprecations carry the line; metaphone() stops at NUL
--FILE--
<?php
// Diagnostics of internal functions report the calling line
function f() {
    return [pow(0, -1), utf8_encode("\xe9"), bin2hex(utf8_decode("\xc3\xa9"))];
}
var_dump(f());
// metaphone() stops at the first NUL byte like the C string walk
var_dump(metaphone("abc\0def"), metaphone("Thompson\0x"), soundex("Robert"));
--EXPECTF--

Deprecated: Power of base 0 and negative exponent is deprecated in %s on line 4

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 4

Deprecated: Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 4
array(3) {
  [0]=>
  float(INF)
  [1]=>
  string(2) "é"
  [2]=>
  string(2) "e9"
}
string(3) "ABK"
string(5) "0MPSN"
string(4) "R163"
