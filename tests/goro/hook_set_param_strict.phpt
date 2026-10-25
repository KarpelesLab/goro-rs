--TEST--
Property hooks: the set hook's parameter type under strict_types
--FILE--
<?php
declare(strict_types=1);
class A { public int $x = 1 { set(int $v) { $this->x = $v; } } public int $y = 1 { set => $value; } public int $z { get=>1; set{} } }
$a = new A;
try { $a->x = "5"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $a->y = "5"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $a->z = "5"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$a->x = 5; $a->y = 6; var_dump($a->x, $a->y);
--EXPECTF--
A::$x::set(): Argument #1 ($v) must be of type int, string given, called in %s on line 5
A::$y::set(): Argument #1 ($value) must be of type int, string given, called in %s on line 6
A::$z::set(): Argument #1 ($value) must be of type int, string given, called in %s on line 7
int(5)
int(6)
