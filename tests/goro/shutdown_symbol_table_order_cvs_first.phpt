--TEST--
Shutdown destructors walk the symbol table with the main script's variables attached first
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "destruct {$this->n}\n"; } }
class S1 { static $a; static $b; }
class S2 { static $c; }
S2::$c = new D('S2c');
S1::$b = new D('S1b');
S1::$a = new D('S1a');
$arr = ['x' => new D('arr-x'), 'y' => [new D('arr-y-0')]];
$cl = function() { static $st; $st = new D('closure-static'); };
$cl();
$cap = new D('captured');
$fn = function() use ($cap) { return $cap; };
function gen() { $g = new D('gen-local'); yield 1; yield 2; }
$gen = gen(); $gen->current();
$o1 = new D('o1'); $o2 = new D('o2'); $o1->other = $o2;
$ref = new D('ref'); $r2 = &$ref;
$GLOBALS['late'] = new D('late');
register_shutdown_function(function () { echo "shutdown1\n"; $GLOBALS['fromshutdown'] = new D('fromshutdown'); });
class Thrower { function __destruct() { echo "thrower\n"; throw new Exception("from dtor"); } }

$z = new D('z-last');
echo "end of script\n";
--EXPECTF--

Deprecated: Creation of dynamic property D::$other is deprecated in %s on line 15
end of script
shutdown1
destruct fromshutdown
destruct late
destruct z-last
destruct o1
destruct gen-local
destruct captured
destruct closure-static
destruct o2
destruct S2c
destruct S1b
destruct S1a
destruct arr-x
destruct arr-y-0
destruct ref
