--TEST--
A static property sent to an array by-reference parameter of a builtin is not auto-initialized
--FILE--
<?php
class K { public static $s = [3, [4]]; public static ?array $t = null; }
K::$s = null;
try { $y = array_pop(K::$s); var_dump($y); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(K::$s);
try { sort(K::$s); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
K::$s = 5;
try { array_push(K::$s, 1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$o = new stdClass; $o->p = null;
try { array_pop($o->p); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$arr = ['x' => null];
try { array_pop($arr['x']); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_shift(K::$t); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
trait T { public static $x = null; }
try { array_pop(T::$x); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
class P { private static $priv = null; }
try { array_pop(P::$priv); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { array_pop(P::$nope); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Q { public static ?array $t; }
try { array_pop(Q::$t); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class R { public static $a = [1, 2]; }
var_dump(array_pop(R::$a), R::$a);
--EXPECTF--
array_pop(): Argument #1 ($array) must be of type array, null given
NULL
sort(): Argument #1 ($array) must be of type array, null given
array_push(): Argument #1 ($array) must be of type array, int given
array_pop(): Argument #1 ($array) must be of type array, null given
array_pop(): Argument #1 ($array) must be of type array, null given
array_shift(): Argument #1 ($array) must be of type array, null given

Deprecated: Accessing static trait property T::$x is deprecated, it should only be accessed on a class using the trait in %s on line 15
array_pop(): Argument #1 ($array) must be of type array, null given
Cannot access private property P::$priv
Access to undeclared static property P::$nope
array_pop(): Argument #1 ($array) must be of type array, null given
int(2)
array(1) {
  [0]=>
  int(1)
}
