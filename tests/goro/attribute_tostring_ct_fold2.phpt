--TEST--
Attribute arguments fold internal and earlier-declared class constants; parameter defaults do not
--FILE--
<?php
const FOO = 'foo';
class K { const B = 3; const C = self::B + 1; }
function h($x = PHP_INT_SIZE, $y = K::B, $z = K::C, $w = E_ALL, $v = PHP_EOL, $u = M_PI, $t = PHP_OS, $s = \PHP_VERSION_ID > 1, $r = K::class) {}
echo new ReflectionFunction('h');
#[Attribute]
class A { function __construct(...$a) {} }
#[A(K::C, E_ALL, PHP_EOL, M_PI, PHP_OS, \PHP_INT_MAX, L::X, SORT_STRING|SORT_FLAG_CASE)]
function f() {}
class L { const X = 1; }
echo (new ReflectionFunction('f'))->getAttributes()[0];
--EXPECTF--
Function [ <user> function h ] {
  @@ %s 4 - 4

  - Parameters [9] {
    Parameter #0 [ <optional> $x = PHP_INT_SIZE ]
    Parameter #1 [ <optional> $y = K::B ]
    Parameter #2 [ <optional> $z = K::C ]
    Parameter #3 [ <optional> $w = E_ALL ]
    Parameter #4 [ <optional> $v = PHP_EOL ]
    Parameter #5 [ <optional> $u = M_PI ]
    Parameter #6 [ <optional> $t = PHP_OS ]
    Parameter #7 [ <optional> $s = PHP_VERSION_ID > 1 ]
    Parameter #8 [ <optional> $r = 'K' ]
  }
}
Attribute [ A ] {
  - Arguments [8] {
    Argument #0 [ 4 ]
    Argument #1 [ 30719 ]
    Argument #2 [ '\n' ]
    Argument #3 [ 3.1415926535898 ]
    Argument #4 [ 'Linux' ]
    Argument #5 [ 9223372036854775807 ]
    Argument #6 [ L::X ]
    Argument #7 [ 10 ]
  }
}
