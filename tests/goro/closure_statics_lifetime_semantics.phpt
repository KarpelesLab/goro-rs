--TEST--
Closure static variables: IIFE, generators, bound copies, recursion
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "destruct {$this->n}\n"; } }
// immediately invoked closures with statics
for ($i = 0; $i < 3; $i++) {
    echo (function () { static $n = 0; $o = new D("iife"); return ++$n; })(), "\n";
}
// generator closure holding statics
$g = (function () { static $s = 10; $s++; yield $s; $s++; yield $s; })();
foreach ($g as $v) { echo "g=$v\n"; }
$mk = function () { static $c = 0; return ++$c; };
$fns = [];
for ($i = 0; $i < 3; $i++) { $fns[] = $mk->bindTo(null); }
foreach ($fns as $f) echo $f(), $f(), "\n";
$fns = null;
echo $mk(), $mk(), "\n";
function holder() { static $keep; $keep ??= function () { static $x = 0; return ++$x; }; return $keep; }
echo holder()(), holder()(), holder()(), "\n";
$rec = function ($n) use (&$rec) { static $calls = 0; $calls++; return $n > 0 ? $rec($n - 1) : $calls; };
echo $rec(4), "\n";
$arr = array_map(function ($x) { static $t = 0; return $x + ++$t; }, [10, 20, 30]);
echo implode(",", $arr), "\n";
echo "end\n";
--EXPECT--
destruct iife
1
destruct iife
1
destruct iife
1
g=11
g=12
12
12
12
12
123
5
11,22,33
end
