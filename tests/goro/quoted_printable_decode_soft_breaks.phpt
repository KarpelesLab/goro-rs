--TEST--
quoted_printable_decode(): soft line breaks, trailing '=', stops at NUL
--FILE--
<?php
foreach (["ab\0cd", "a=", "a=\r", "a=\n", "a= \r\nb", "a=\t \nb", "a=\r\rb", "a=41=4a=4", "a=4", "=", "==41", "a=  ", "a= x", "a=\r\n", "=0A=0d", "x=G1", "=\n=\r\n=", ""] as $s) {
  echo bin2hex(quoted_printable_decode($s)), "\n";
}
--EXPECT--
6162
61
61
61
6162
6162
610d62
61414a3d34
613d34

3d41
61
613d2078
61
0a0d
783d4731
