--TEST--
DivisionByZeroError thrown by / and % carries its file, line and trace
--FILE--
<?php
function f($a, $b) { return $a / $b; }
function g($a, $b) { return $a % $b; }
try { var_dump(1 % 0); } catch (\Throwable $e) { var_dump($e->getLine(), $e->getFile() == __FILE__); }
try { var_dump(1 / 0); } catch (\Throwable $e) { var_dump($e->getLine()); }
try { f(1, 0); } catch (\Throwable $e) { var_dump($e->getLine(), $e->getTrace()[0]['function']); }
try { g(1, 0); } catch (\Throwable $e) { var_dump($e->getLine(), $e->getTrace()[0]['function']); }
--EXPECTF--
int(4)
bool(true)
int(5)
int(2)
string(1) "f"
int(3)
string(1) "g"
