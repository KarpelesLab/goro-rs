--TEST--
Multi-line calls: SEND, property fetch and special builtin opcodes on their own lines
--FILE--
<?php
function f($a, $b) {}
f(
  $undef1,
  $undef2
);
function g(int $x) {}
try { g(
  "abc"
); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
strlen(
  $u3
);
$o = new stdClass;
f(
  $o->nope,
  $o
    ->nope2
);
--EXPECTF--

Warning: Undefined variable $undef1 in %s on line 4

Warning: Undefined variable $undef2 in %s on line 5
g(): Argument #1 ($x) must be of type int, string given, called in %s on line 8

Warning: Undefined variable $u3 in %s on line 12

Deprecated: strlen(): Passing null to parameter #1 ($string) of type string is deprecated in %s on line 12

Warning: Undefined property: stdClass::$nope in %s on line 16

Warning: Undefined property: stdClass::$nope2 in %s on line 18
