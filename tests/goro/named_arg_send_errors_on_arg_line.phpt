--TEST--
Named argument errors are raised at the argument's send, on its line
--FILE--
<?php
function g($a = 1, $b = 2) { var_dump($a); }
try {
g(
  1,
  b: 2,
  a: 3
);
} catch (Error $e) { echo $e->getMessage(), " ", $e->getLine(), "\n"; }
try {
g(
  a: 1,
  b: 2,
  a: print("side\n")
);
} catch (Error $e) { echo $e->getMessage(), " ", $e->getLine(), "\n"; }
try {
strlen(
  string: 1,
  string: 3
);
} catch (Error $e) { echo $e->getMessage(), " ", $e->getLine(), "\n"; }
class C { function m($a) {} static function s($a) {} }
try {
(new C)->m(
  a: 1,
  a: 3
);
} catch (Error $e) { echo $e->getMessage(), " ", $e->getLine(), "\n"; }
try {
C::s(
  1,
  a: 3
);
} catch (Error $e) { echo $e->getMessage(), " ", $e->getLine(), "\n"; }
try {
new C(
  a: 1,
  a: 3
);
} catch (Error $e) { echo $e->getMessage(), " ", $e->getLine(), "\n"; }
--EXPECT--
Named parameter $a overwrites previous argument 7
side
Named parameter $a overwrites previous argument 14
Named parameter $string overwrites previous argument 20
Named parameter $a overwrites previous argument 27
Named parameter $a overwrites previous argument 33
Unknown named parameter $a 38
