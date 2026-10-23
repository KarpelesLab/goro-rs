--TEST--
pack(): codes and counts are checked before any argument is converted
--FILE--
<?php
function t($f) { try { $r = $f(); echo bin2hex($r), "\n"; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => pack('H3C2', NAN, 255));
t(fn() => pack('Q@n2H5', 1.5, INF, -2.5, NAN));
t(fn() => pack('n', new stdClass));
t(fn() => pack('f', new stdClass));
t(fn() => pack('d', new stdClass));
t(fn() => pack('a', new stdClass));
t(fn() => pack('C99999999999999999999', 1));
t(fn() => pack('C*', 1, 2, 3));
t(fn() => pack('nC', 1, 2, 3, 4));
t(fn() => pack('n2X5', 1, NAN));
t(fn() => pack('A3', NAN));
t(fn() => pack('Z*a2', 1.5, INF));
t(fn() => pack('h3', "zz"));
t(fn() => pack('q', "abc"));
t(fn() => pack('J', [1]));
t(fn() => pack('y', 1));
t(fn() => pack('N', 1.5, 2));
t(fn() => pack('@-1'));
t(fn() => pack('x2X3@2'));
t(fn() => pack('a*', 1, 2));
t(fn() => pack('Ca', 300, "q"));
t(fn() => pack('e', NAN));
t(fn() => pack('C4294967297', 1));
t(fn() => pack('C2147483648', 1));
t(fn() => pack('x99999999999999999999'));
t(fn() => pack('a2147483648', "xy"));
t(fn() => pack('a*', new stdClass));
t(fn() => pack('a*C', new stdClass));
t(fn() => pack('aC', new stdClass));
t(fn() => pack('nC*', new stdClass, 1.5e30, "7"));
--EXPECTF--
ValueError: Type C: too few arguments

Warning: The float INF is not representable as an int, cast occurred in %s on line 4

Warning: unexpected NAN value was coerced to string in %s on line 4

Warning: pack(): Type H: not enough characters in string in %s on line 4

Warning: pack(): Type H: illegal hex digit N in %s on line 4

Warning: pack(): Type H: illegal hex digit N in %s on line 4
010000fffe0a00

Warning: Object of class stdClass could not be converted to int in %s on line 5
0001

Warning: Object of class stdClass could not be converted to float in %s on line 6
0000803f

Warning: Object of class stdClass could not be converted to float in %s on line 7
000000000000f03f
Error: Object of class stdClass could not be converted to string
01
010203

Warning: pack(): 2 arguments unused in %s on line 11
000102

Warning: pack(): Type X: outside of string in %s on line 12

Warning: The float NAN is not representable as an int, cast occurred in %s on line 12


Warning: unexpected NAN value was coerced to string in %s on line 13
4e414e
312e3500494e

Warning: pack(): Type h: not enough characters in string in %s on line 15

Warning: pack(): Type h: illegal hex digit z in %s on line 15

Warning: pack(): Type h: illegal hex digit z in %s on line 15
00
0000000000000000
0000000000000001
ValueError: Type y: unknown format code

Warning: pack(): 1 arguments unused in %s on line 19
00000001
ValueError: Type -: unknown format code

Warning: pack(): Type X: outside of string in %s on line 21
0000

Warning: pack(): 1 arguments unused in %s on line 22
31
2c71
000000000000f87f
01
01

Warning: pack(): Type x: '*' ignored in %s on line 27
00
7879
Error: Object of class stdClass could not be converted to string
Error: Object of class stdClass could not be converted to string
ValueError: Type C: too few arguments

Warning: Object of class stdClass could not be converted to int in %s on line 32

Warning: The float 1.5E+30 is not representable as an int, cast occurred in %s on line 32
00010007
