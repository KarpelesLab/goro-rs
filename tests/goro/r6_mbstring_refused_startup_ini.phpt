--TEST--
mbstring: INI values the handlers refuse at startup give way to the defaults
--INI--
mbstring.language=bogus
mbstring.detect_order=bogus
mbstring.http_input=bogus
mbstring.http_output=bogus
--FILE--
<?php
foreach (['mbstring.language', 'mbstring.detect_order', 'mbstring.http_input', 'mbstring.http_output', 'mbstring.internal_encoding', 'mbstring.substitute_character'] as $k) var_dump(ini_get($k));
var_dump(mb_language(), mb_detect_order(), mb_http_input('I'), mb_http_output(), mb_internal_encoding(), mb_substitute_character());
?>
--EXPECT--

Warning: PHP Startup: INI setting contains invalid encoding "bogus" in Unknown on line 0

Deprecated: PHP Startup: Use of mbstring.http_input is deprecated in Unknown on line 0

Warning: PHP Startup: INI setting contains invalid encoding "bogus" in Unknown on line 0

Deprecated: PHP Startup: Use of mbstring.http_output is deprecated in Unknown on line 0
string(7) "neutral"
string(0) ""
string(0) ""
string(0) ""
string(0) ""
string(0) ""
string(7) "neutral"
array(2) {
  [0]=>
  string(5) "ASCII"
  [1]=>
  string(5) "UTF-8"
}
array(1) {
  [0]=>
  string(5) "UTF-8"
}
string(5) "UTF-8"
string(5) "UTF-8"
int(63)
