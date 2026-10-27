--TEST--
Compound assignments and ++/-- on typed properties holding a reference report the reference
--FILE--
<?php
class T { public int $i = 5; }
$t = new T;
$r = &$t->i;
try { $t->i .= "x"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
$t->i += 1.5;
var_dump($t->i);
try { $t->i = "abc"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
$t->i = PHP_INT_MAX;
try { $t->i += 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $t->i++; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($t->i);
class S { public static int $si = 1; }
$sr = &S::$si;
try { S::$si .= "x"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { S::$si = "x"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
S::$si *= 2.0;
var_dump(S::$si);
S::$si = PHP_INT_MAX;
try { S::$si++; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { --$sr; S::$si = PHP_INT_MIN; S::$si--; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(S::$si);
--EXPECTF--
Cannot assign string to reference held by property T::$i of type int

Deprecated: Implicit conversion from float 6.5 to int loses precision in %s on line 6
int(6)
Cannot assign string to property T::$i of type int
Cannot assign float to reference held by property T::$i of type int
Cannot increment a reference held by property T::$i of type int past its maximal value
int(9223372036854775807)
Cannot assign string to reference held by property S::$si of type int
Cannot assign string to property S::$si of type int
int(2)
Cannot increment a reference held by property S::$si of type int past its maximal value
Cannot decrement a reference held by property S::$si of type int past its minimal value
int(-9223372036854775808)
