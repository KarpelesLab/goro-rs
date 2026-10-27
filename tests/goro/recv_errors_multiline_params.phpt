--TEST--
Argument errors are raised on the line of the parameter (RECV), in multi-line parameter lists
--FILE--
<?php
function f(
    int $a,
    int
    $b,
    $c = 5,
    int ...$d
) {}
try { f(); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { f(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { f("x", 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { f(1, "x"); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { f(1, 2, 3, 4, "x"); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
$cl = function (
    $a,
    $b
) {};
try { $cl(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
class K {
    function m(
        $a,
        array $b
    ) {}
}
try { (new K)->m(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { (new K)->m(1, 2); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
function g(
    $a,
    $b
): int { return "x"; }
try { g(1, 2); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
function h(
    int $a = 5,
    $b = new stdClass,
    string $c = "1",
) { var_dump($c); }
h();
try { h(c: []); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
--EXPECTF--
Too few arguments to function f(), 0 passed in %s on line 9 and at least 2 expected @3
Too few arguments to function f(), 1 passed in %s on line 10 and at least 2 expected @4
f(): Argument #1 ($a) must be of type int, string given, called in %s on line 11 @3
f(): Argument #2 ($b) must be of type int, string given, called in %s on line 12 @4
f(): Argument #5 must be of type int, string given, called in %s on line 13 @7
Too few arguments to function {closure:%s:14}(), 1 passed in %s on line 18 and exactly 2 expected @16
Too few arguments to function K::m(), 1 passed in %s on line 25 and exactly 2 expected @22
K::m(): Argument #2 ($b) must be of type array, int given, called in %s on line 26 @22
g(): Return value must be of type int, string returned @30
string(1) "1"
h(): Argument #3 ($c) must be of type string, array given, called in %s on line 38 @35
