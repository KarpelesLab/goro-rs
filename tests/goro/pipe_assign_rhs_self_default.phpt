--TEST--
An assignment on the right of |> / a binary operator; self:: in a free function's default fails when evaluated
--FILE--
<?php
try { 1 |> $x = 5; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($x);
try { $r = 2 |> $f = strval(...); var_dump($r); } catch (Error $e) { echo $e->getMessage(), "\n"; }
function fx($a = self::B) { return $a; }
try { fx(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function fy($a = parent::B) { return $a; }
try { fy(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(1 + $x = 5, $x);
var_dump(2 . $y .= "a", $y);
var_dump(!$z = 0);
var_dump(3 |> $f = fn($v) => $v * 2);
?>
--EXPECTF--
Error: Value of type int is not callable
int(5)
string(1) "2"
Error: Cannot access "self" when no class scope is active
Error: Cannot access "parent" when no class scope is active
int(6)
int(5)

Warning: Undefined variable $y in %s on line 10
string(2) "2a"
string(1) "a"
bool(true)
int(6)
