--TEST--
A Generator object is an illegal array offset; array_walk() walks its (no) properties
--FILE--
<?php
function g() { yield 1; }
$gen = g();
try { $a = [$gen => 1]; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $a = []; $a[$gen] = 1; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $a = [1]; echo $a[$gen]; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { var_dump(isset([1][$gen])); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(array_walk($gen, fn() => 1));
--EXPECT--
Cannot access offset of type Generator on array
Cannot access offset of type Generator on array
Cannot access offset of type Generator on array
Cannot access offset of type Generator in isset or empty
bool(true)
