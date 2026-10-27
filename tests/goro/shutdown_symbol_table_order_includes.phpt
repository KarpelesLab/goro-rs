--TEST--
Shutdown destructor order with included files, eval, globals and extract
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "destruct {$this->n}\n"; } }
$GLOBALS['dyn1'] = new D('dyn1');
file_put_contents(__DIR__ . '/sd3inc.php', '<?php $inc1 = new D("inc1"); $GLOBALS["dyn2"] = new D("dyn2"); $inc2 = new D("inc2");');
include __DIR__ . '/sd3inc.php';
unlink(__DIR__ . '/sd3inc.php');
$a = new D('a');
function f() { global $g1; $g1 = new D('g1'); $GLOBALS['g2'] = new D('g2'); }
f();
eval('$ev = new D("ev");');
extract(['ex' => new D('ex')]);
$b = new D('b');
${'var' . 'var'} = new D('varvar');
$c = new D('c');
echo "end\n";
--EXPECT--
end
destruct ex
destruct ev
destruct g2
destruct dyn2
destruct inc2
destruct inc1
destruct dyn1
destruct c
destruct varvar
destruct b
destruct a
destruct g1
