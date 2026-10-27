--TEST--
A closure's static variables are freed with the Closure object
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
$fn = function() { static $st; $st = new D('closure-static'); };
$fn();
$fn = null;
echo "a\n";
$fn2 = function() { static $st; $st ??= new D('shared-by-clone'); };
$fn2(); $copy = clone $fn2; $copy();
$fn2 = null;
echo "b\n";
$copy = null;
echo "c\n";
class K { function m() { return function() { static $s; $s = new D('method-closure'); }; } }
$c = (new K)->m(); $c(); unset($c);
echo "d\n";
function regular() { static $r; $r = new D('function-static'); }
regular();
$b = Closure::bind(function() { static $q; $q = new D('bound'); }, null, K::class);
$b(); $b = null;
echo "end\n";
--EXPECT--
dtor closure-static
a
b
dtor shared-by-clone
c
dtor method-closure
d
dtor bound
end
dtor function-static
