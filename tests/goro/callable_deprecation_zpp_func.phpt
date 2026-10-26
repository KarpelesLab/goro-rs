--TEST--
Z_PARAM_FUNC raises callable deprecations once: usort() family, array_reduce(), array_any()/array_find(), iterator_apply()
--FILE--
<?php
class A { function m() { return 1; } static function s() { return 1; } }
$v = [new A, 'A::m'];
echo "map0\n"; array_map($v, []);
echo "map2\n"; array_map($v, [1, 2]);
echo "filter0\n"; array_filter([], $v);
echo "filter2\n"; array_filter([1, 2], $v);
echo "usort0\n"; $a = []; usort($a, $v);
echo "usort1\n"; $a = [1]; usort($a, $v);
echo "usort3\n"; $a = [3, 2, 1]; usort($a, $v);
echo "uasort\n"; $a = [3, 2, 1]; uasort($a, $v);
echo "uksort\n"; $a = [3, 2, 1]; uksort($a, $v);
echo "walk\n"; $a = [3, 2]; array_walk($a, $v);
echo "reduce\n"; array_reduce([1, 2], $v);
echo "cufa\n"; call_user_func_array($v, []);
echo "iter\n"; iterator_apply(new ArrayIterator([1]), $v);
echo "prc\n"; preg_replace_callback('/a/', $v, 'aa');
echo "any\n"; array_any([1, 2], $v);
echo "find\n"; array_find([1, 2], $v);
echo "udiff\n"; array_udiff([1, 2], [3], $v);
echo "cfi\n"; foreach (new CallbackFilterIterator(new ArrayIterator([1, 2]), $v) as $x);
echo "fcc\n"; Closure::fromCallable($v);
echo "is_callable\n"; is_callable($v);
echo "done\n";
$v = [new A, 'A::m'];
var_dump(iterator_apply(new ArrayIterator([1, 2, 3]), $v));
var_dump(iterator_apply(new ArrayIterator([1, 2]), fn() => NAN));
--EXPECTF--
map0

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 4
map2

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 5
filter0

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 6
filter2

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 7
usort0

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 8
usort1

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 9
usort3

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 10
uasort

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 11
uksort

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 12
walk

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 13
reduce

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 14
cufa

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 15
iter

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 16
prc

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 17
any

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 18
find

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 19
udiff

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 20
cfi

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 21
fcc

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 22
is_callable

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 23
done

Deprecated: Callables of the form ["A", "A::m"] are deprecated in %s on line 26
int(3)

Warning: unexpected NAN value was coerced to bool in %s on line 27

Warning: unexpected NAN value was coerced to bool in %s on line 27
int(2)
