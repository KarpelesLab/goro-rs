--TEST--
A closure's static variables are destroyed with the closure object
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "destruct {$this->n}\n"; } }
$cl = function() { static $st; $st = new D('closure-static'); };
$cl();
unset($cl);
echo "after unset\n";
function mk() { return function() { static $n = 0; return ++$n; }; }
$a = mk(); $b = mk();
var_dump($a(), $a(), $b());
$c = clone $a; var_dump($c(), $a());
$fn = fn() => 1;
class K { function m() { return function() { static $s; $s ??= new D('method-closure-static'); }; } }
$k = (new K)->m(); $k(); $k = null;
echo "after k\n";
$bound = Closure::bind(function() { static $q; $q = new D('bound-static'); }, null, null);
$bound(); unset($bound);
echo "end\n";
--EXPECT--
destruct closure-static
after unset
int(1)
int(2)
int(1)
int(3)
int(3)
destruct method-closure-static
after k
destruct bound-static
end
