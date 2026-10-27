--TEST--
Lean calls with typed parameters and returns: scalar types checked by kind, coercions and errors by the full checks
--FILE--
<?php
function fi(int $x) { return var_export($x, true); }
function ff(float $x) { return var_export($x, true); }
function fs(?string $x) { return var_export($x, true); }
function fu(int|string $x) { return var_export($x, true); }
function fb(bool $x) { return var_export($x, true); }
function fa(array $x) { return count($x); }
function fn_(null|false $x) { return var_export($x, true); }
function fr(int $x): float { return $x; }
function frs($x): ?string { return $x; }
function fiter(iterable $x) { return gettype($x); }
function fobj(object $x) { return get_class($x); }
function fmixed(mixed $x) { return gettype($x); }
class P { function m(INT $a, ?Float $b = null): Int { return $a + (int) $b; } }
$p = new P;
for ($i = 0; $i < 3; $i++) {
    echo fi(5), fi("6"), fi(7.0), fi(true), "\n";
    echo ff(1.5), ff(2), ff("3.5"), "\n";
    echo fs("a"), fs(null), fs(5), "\n";
    echo fu(1), fu("x"), fu(2.0), fu(true), "\n";
    echo fb(true), fb(false), fb(0), fb("1"), "\n";
    echo fa([1]), fa([]), "\n";
    echo fn_(null), fn_(false), "\n";
    echo var_export(fr(3), true), frs("s"), var_export(frs(null), true), frs(4), "\n";
    echo fiter([1]), fobj($p), fmixed(null), fmixed(1.5), "\n";
    echo $p->m(1), $p->m(2, 3.5), $p->m(3, 1), $p->m("4"), "\n";
    try { echo fi("x"); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { echo fa(1); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { echo fn_(true); } catch (TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
echo implode(",", array_map('fi', [1, "2", 3.0])), "\n";
echo implode(",", array_map(fn(int $x): int => $x * 2, [1, "2"])), "\n";
echo implode(",", array_map([$p, 'm'], [1, 2], [1.5, null])), "\n";
--EXPECTF--
5671
1.52.03.5
'a'NULL'5'
1'x'21
truefalsefalsetrue
10
NULLfalse
3.0sNULL4
arrayPNULLdouble
1544
TypeError: fi(): Argument #1 ($x) must be of type int, string given, called in %s on line 27
TypeError: fa(): Argument #1 ($x) must be of type array, int given, called in %s on line 28
TypeError: fn_(): Argument #1 ($x) must be of type ?false, true given, called in %s on line 29
5671
1.52.03.5
'a'NULL'5'
1'x'21
truefalsefalsetrue
10
NULLfalse
3.0sNULL4
arrayPNULLdouble
1544
TypeError: fi(): Argument #1 ($x) must be of type int, string given, called in %s on line 27
TypeError: fa(): Argument #1 ($x) must be of type array, int given, called in %s on line 28
TypeError: fn_(): Argument #1 ($x) must be of type ?false, true given, called in %s on line 29
5671
1.52.03.5
'a'NULL'5'
1'x'21
truefalsefalsetrue
10
NULLfalse
3.0sNULL4
arrayPNULLdouble
1544
TypeError: fi(): Argument #1 ($x) must be of type int, string given, called in %s on line 27
TypeError: fa(): Argument #1 ($x) must be of type array, int given, called in %s on line 28
TypeError: fn_(): Argument #1 ($x) must be of type ?false, true given, called in %s on line 29
1,2,3
2,4
2,2
