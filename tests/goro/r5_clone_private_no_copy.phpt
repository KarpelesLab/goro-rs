--TEST--
A clone whose __clone() is inaccessible copies nothing (no object id used)
--FILE--
<?php
class Priv { private function __clone() {} }
try { clone new Priv; } catch (Error $e) { echo spl_object_id($e), "\n"; }
var_dump(spl_object_id(new stdClass));
function gen() { yield 1; }
try { $g = gen(); $x = clone $g; } catch (Error $e) { echo spl_object_id($e), " ", spl_object_id($g), "\n"; }
var_dump(spl_object_id(new stdClass));
--EXPECT--
2
int(1)
3 1
int(2)
