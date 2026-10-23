--TEST--
ReflectionParameter offsets of internal functions, object offset type names, unserialize E: of internal classes, serialize(-0.0), __sleep objects, var_export of anonymous classes
--FILE--
<?php
function t($l, $f) { try { $r = $f(); echo "$l: "; var_dump($r); } catch (\Throwable $e) { echo "$l: EXC ", get_class($e), ": ", $e->getMessage(), "\n"; } }
$c = function () {};
t(1, function () use ($c) { is_callable($c, false, $n); return $n; });
t(2, function () { is_callable(fn() => 1, true, $n); return $n; });
t(3, fn() => new ReflectionParameter('strlen', 5));
t(4, fn() => new ReflectionParameter('strlen', 0) instanceof ReflectionParameter);
t(5, fn() => unserialize('E:13:"ArrayObject:A";'));
t(6, function () { ini_set('serialize_precision', 0); $r = serialize(-0.0); ini_set('serialize_precision', -1); return $r; });
t(7, fn() => (new ArrayObject([]))[new stdClass]);
t(8, function () { $a = new ArrayObject([]); $a[new stdClass] = 1; });
t(9, fn() => iterator_to_array((function () { yield new stdClass => 1; })()));
t(10, fn() => (new ArrayObject([1]))[[]] ?? 'x');
t(11, fn() => isset((new ArrayObject([1]))[new stdClass]));
class SL { function __sleep() { return [new stdClass]; } }
t(12, fn() => serialize(new SL));
t(13, function () { $o = new class { public $a = 1; }; return str_contains(var_export($o, true), "\0") ? 'nul' : var_export($o, true); });
--EXPECTF--
1: string(120) "{closure:%s:3}"
2: string(132) "{closure:{closure:%s:5}:5}"
3: EXC ReflectionException: The parameter specified by its offset could not be found
4: bool(true)

Warning: unserialize(): Class 'ArrayObject' is not an enum in %s on line 8

Warning: unserialize(): Error at offset 0 of 21 bytes in %s on line 8
5: bool(false)
6: string(10) "d:-0.0E+0;"
7: EXC TypeError: Cannot access offset of type stdClass on ArrayObject
8: EXC TypeError: Cannot access offset of type stdClass on ArrayObject
9: EXC TypeError: Cannot access offset of type stdClass on array
10: EXC TypeError: Cannot access offset of type array in isset or empty
11: EXC TypeError: Cannot access offset of type stdClass in isset or empty

Warning: serialize(): SL::__sleep() should return an array only containing the names of instance-variables to serialize in %s on line 16
12: EXC Error: Object of class stdClass could not be converted to string
13: string(3) "nul"
