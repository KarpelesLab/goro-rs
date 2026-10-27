--TEST--
escapeshellarg()/escapeshellcmd() drop characters invalid in the LC_CTYPE (php_mblen())
--FILE--
<?php
foreach (["C", "C.UTF-8", "POSIX"] as $l) {
  var_dump(setlocale(LC_CTYPE, $l));
  var_dump(bin2hex(escapeshellarg("a\xffb\xc3\xa9c\xe2\x82")), bin2hex(escapeshellcmd("a\xff;b\xc3\xa9'c\xe2")));
}
--EXPECT--
string(1) "C"
string(10) "2761626327"
string(14) "615c3b625c2763"
string(7) "C.UTF-8"
string(14) "276162c3a96327"
string(18) "615c3b62c3a95c2763"
string(1) "C"
string(10) "2761626327"
string(14) "615c3b625c2763"
