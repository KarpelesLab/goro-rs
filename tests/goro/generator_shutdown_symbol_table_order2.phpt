--TEST--
Suspended generators and objects in globals are destroyed in reverse declaration order at shutdown
--FILE--
<?php
class Dd { function __construct(public $n) {} function __destruct() { echo "Dd{$this->n} dtor\n"; } }
function endg($n) { try { yield 1; } finally { echo "gen$n finally\n"; } }
$a = new Dd(1);
$e = endg(1); $e->current();
$b = new Dd(2);
$f = endg(2); $f->current();
$c = new Dd(3);
$shared = new Dd(4); $s2 = $shared;
echo "script end\n";
--EXPECT--
script end
Dd3 dtor
gen2 finally
Dd2 dtor
gen1 finally
Dd1 dtor
Dd4 dtor
