--TEST--
Scalar literal operations fold at compile time, so commutative operands keep their order
--FILE--
<?php
try { $tmp = $i; $tmp /= ([1] ^ (true + -1)); show($tmp); } catch (\Throwable $ex) { echo get_class($ex), ': ', $ex->getMessage(), "\n"; }
try { $tmp = $s; $tmp %= (("9.5x" != INF) & implode(",", (array)[])); show($tmp); } catch (\Throwable $ex) { echo get_class($ex), ': ', $ex->getMessage(), "\n"; }
try { $tmp = $ns; $tmp <<= (1.5 | (true * INF)); show($tmp); } catch (\Throwable $ex) { echo get_class($ex), ': ', $ex->getMessage(), "\n"; }
--EXPECTF--

Warning: Undefined variable $i in %s on line 2
TypeError: Unsupported operand types: array ^ int

Warning: Undefined variable $s in %s on line 3
TypeError: Unsupported operand types: string & bool

Warning: Undefined variable $ns in %s on line 4

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 4

Warning: The float INF is not representable as an int, cast occurred in %s on line 4
Error: Call to undefined function show()
