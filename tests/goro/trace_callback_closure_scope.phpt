--TEST--
A closure called back by an internal function is qualified by its scope in backtraces and messages
--FILE--
<?php
class A {
    function m() { return array_map(function($y) { $t = debug_backtrace(); return [$t[0]['function'], $t[0]['class'] ?? '-', $t[0]['type'] ?? '-', func_get_args()]; }, [7]); }
    function typed() { return array_map(function(int $y) { return $y; }, ["abc"]); }
    static function st() { return array_map(fn(int $y) => $y, ["x"]); }
    function byref() { $a = [1]; array_map(function(&$v) { $v = 2; }, $a); }
}
print_r((new A)->m());
try { (new A)->typed(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { A::st(); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
(new A)->byref();
$c = Closure::bind(function() { return array_map(function() { throw new Exception("bound"); }, [1]); }, new A, A::class);
try { $c(); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
$d = Closure::bind(function() { return array_map(function() { throw new Exception("scope only"); }, [1]); }, null, A::class);
try { $d(); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
usort($arr, function($a, $b) { return 0; });
$arr = [3, 1];
class B { function sort(&$arr) { usort($arr, function($a, $b) { debug_print_backtrace(); return $a <=> $b; }); } }
(new B)->sort($arr);
--EXPECTF--
Array
(
    [0] => Array
        (
            [0] => {closure:A::m():3}
            [1] => A
            [2] => ->
            [3] => Array
                (
                    [0] => 7
                )

        )

)
A::{closure:A::typed():4}(): Argument #1 ($y) must be of type int, string given
#0 [internal function]: A->{closure:A::typed():4}('abc')
#1 %s(4): array_map(Object(Closure), Array)
#2 %s(9): A->typed()
#3 {main}
A::{closure:A::st():5}(): Argument #1 ($y) must be of type int, string given
#0 [internal function]: A::{closure:A::st():5}('x')
#1 %s(5): array_map(Object(Closure), Array)
#2 %s(10): A::st()
#3 {main}

Warning: A::{closure:A::byref():6}(): Argument #1 ($v) must be passed by reference, value given in %s on line 6
#0 [internal function]: A->{closure:{closure:%s:12}:12}(1)
#1 %s(12): array_map(Object(Closure), Array)
#2 %s(13): A->{closure:%s:12}()
#3 {main}
#0 [internal function]: A::{closure:{closure:%s:14}:14}(1)
#1 %s(14): array_map(Object(Closure), Array)
#2 %s(15): A::{closure:%s:14}()
#3 {main}

Fatal error: Uncaught TypeError: usort(): Argument #1 ($array) must be of type array, null given in %s:16
Stack trace:
#0 %s(16): usort(NULL, Object(Closure))
#1 {main}
  thrown in %s on line 16
