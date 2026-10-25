--TEST--
A named argument SEND_UNPACK sends to a function without that parameter fails at once
--FILE--
<?php
function e($c) { try { var_dump($c()); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$u = 5;
e(fn() => (function($a) use ($u) { return $a + $u; })(...['a' => 1]));
e(fn() => (function($a) use ($u) { return $a + $u; })(...['u' => 1]));
class K { function m($x) { return $x; } function __call($n, $a) { return $a; } static function s($y) { return $y; } }
e(fn() => (new K)->m(...['x' => 1]));
e(fn() => (new K)->m(...['z' => 1]));
e(fn() => (new K)->magic(...['z' => 1]));
e(fn() => K::s(...['y' => 2]));
e(fn() => K::s(...['q' => 2]));
function va($a, ...$r) { return [$a, $r]; }
e(fn() => va(...['a' => 1, 'zz' => 2]));
e(fn() => sprintf(...['format' => '%s', 'x' => 2]));
e(fn() => str_replace(...['search' => 'a', 'replace' => 'b', 'subject' => 'aa']));
e(fn() => str_replace(...['search' => 'a', 'replace' => 'b', 'bogus' => 'aa']));
e(fn() => (new K)->m(...['x' => 1, 'x2' => 3]));
e(fn() => array_map(...['callback' => null, 'array' => [1], 'arrays' => [2]]));
e(fn() => va(1, ...['zz' => 2]));
function two($a, $b) { return $a . $b; }
e(fn() => two(...['b' => 1, 'a' => 2]));
e(fn() => two(...[1, 'b' => 2, 'c' => 3]));
e(fn() => two(...new ArrayIterator(['b' => 1, 'c' => 2])));
--EXPECTF--
int(6)
Error: Unknown named parameter $u
int(1)
Error: Unknown named parameter $z
array(1) {
  ["z"]=>
  int(1)
}
int(2)
Error: Unknown named parameter $q
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    ["zz"]=>
    int(2)
  }
}
ArgumentCountError: sprintf() does not accept unknown named parameters
string(2) "bb"
Error: Unknown named parameter $bogus
Error: Unknown named parameter $x2
ArgumentCountError: array_map() does not accept unknown named parameters
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    ["zz"]=>
    int(2)
  }
}
string(2) "21"
Error: Unknown named parameter $c
Error: Unknown named parameter $c
