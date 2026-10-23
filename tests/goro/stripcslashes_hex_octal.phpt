--TEST--
stripcslashes(): backslash-x without hex digits, octal wrapping
--FILE--
<?php
foreach (['\x', 'a\xg', '\777', '\400', '\x41\x4', '\101\1012', '\q', 'a\\', '\\\\', '\e', '\08', '\xfff'] as $s) {
  echo bin2hex(stripcslashes($s)), "\n";
}
--EXPECT--
78
617867
ff
00
4104
414132
71
615c
5c
65
0038
ff66
