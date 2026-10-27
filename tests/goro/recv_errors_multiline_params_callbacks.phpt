--TEST--
Argument errors of callbacks, constructors, arrow functions and generators: the parameter's line
--FILE--
<?php
$cb = function (
    $a,
    int $b
) { return $a; };
try { array_map($cb, [1]); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { array_map($cb, [1], ["x"]); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { call_user_func($cb, 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { call_user_func($cb, 1, "y"); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { $cb(b: 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { (new ReflectionFunction($cb))->invoke(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { (new ReflectionFunction($cb))->invoke(1, []); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
class K {
    public function __construct(
        public int $x,
        public string $y,
    ) {}
    public static function s(
        $a,
        array $b,
    ) {}
}
try { new K(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { new K("a", "b"); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { K::s(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { K::s(1, 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { $k = 'K'; $k::s(1, 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { [new K(1, 'a'), 's'](1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
function fl(
    $a,
    int $b
) {}
fl(1, 1.5);
fl(1, "1.5");
$fn = fn(
    $x,
    int $y
) => $x;
try { $fn(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { $fn(1, []); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
function gen(
    $a,
    int $b
) { yield 1; }
try { gen(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { gen(1, "z"); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
function byref(
    $a,
    &$b
) {}
try { byref(1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
fl(1);
--EXPECTF--
Too few arguments to function {closure:%s:2}(), 1 passed and exactly 2 expected @4
{closure:%s:2}(): Argument #2 ($b) must be of type int, string given @4
Too few arguments to function {closure:%s:2}(), 1 passed in %s on line 8 and exactly 2 expected @4
{closure:%s:2}(): Argument #2 ($b) must be of type int, string given, called in %s on line 9 @4
{closure:%s:2}(): Argument #1 ($a) not passed @3
Too few arguments to function {closure:%s:2}(), 1 passed and exactly 2 expected @4
{closure:%s:2}(): Argument #2 ($b) must be of type int, array given @4
Too few arguments to function K::__construct(), 1 passed in %s on line 23 and exactly 2 expected @16
K::__construct(): Argument #1 ($x) must be of type int, string given, called in %s on line 24 @15
Too few arguments to function K::s(), 1 passed in %s on line 25 and exactly 2 expected @20
K::s(): Argument #2 ($b) must be of type array, int given, called in %s on line 26 @20
K::s(): Argument #2 ($b) must be of type array, int given, called in %s on line 27 @20
Too few arguments to function K::s(), 1 passed in %s on line 28 and exactly 2 expected @20

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 31

Deprecated: Implicit conversion from float-string "1.5" to int loses precision in %s on line 31
Too few arguments to function {closure:%s:35}(), 1 passed in %s on line 39 and exactly 2 expected @37
{closure:%s:35}(): Argument #2 ($y) must be of type int, array given, called in %s on line 40 @37
Too few arguments to function gen(), 1 passed in %s on line 45 and exactly 2 expected @43
gen(): Argument #2 ($b) must be of type int, string given, called in %s on line 46 @43
Too few arguments to function byref(), 1 passed in %s on line 51 and exactly 2 expected @49

Fatal error: Uncaught ArgumentCountError: Too few arguments to function fl(), 1 passed in %s on line 52 and exactly 2 expected in %s:31
Stack trace:
#0 %s(52): fl(1)
#1 {main}
  thrown in %s on line 31
