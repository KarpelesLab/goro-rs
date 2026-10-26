--TEST--
object parameter/return/property types accept a Generator
--FILE--
<?php
function g() { yield 1; }
function f(object $o): object { return $o; }
function h(?object $o = null) { return get_class($o); }
function k(object|array $o) { return gettype($o); }
class P { public object $p; public ?object $q = null; }
$gen = g();
var_dump(is_object($gen), get_class(f($gen)), h($gen), k($gen));
$p = new P; $p->p = $gen; $p->q = $gen;
var_dump($p->p === $gen);
$c = function (object ...$a) { return count($a); };
var_dump($c($gen, $gen));
var_dump((fn(): object => g())() instanceof Generator);

?>
--EXPECT--
bool(true)
string(9) "Generator"
string(9) "Generator"
string(6) "object"
bool(true)
int(2)
bool(true)
