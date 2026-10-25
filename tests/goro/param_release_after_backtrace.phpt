--TEST--
A parameter a backtrace copied is released when the function reassigns it
--FILE--
<?php
class D { public $n; function __construct($n){$this->n=$n;} function __destruct(){ echo "dtor {$this->n}\n"; } }
function t1($f){ $g = [$f]; $f = null; echo "t1 body\n"; }
t1(new D(1)); echo "after t1\n";
function t2($f){ $g = new D(20); $g->x = $f; $f = null; echo "t2 body\n"; }
t2(new D(2)); echo "after t2\n";
function t3($f){ try { throw new Exception; } catch (Exception $e) { $f = null; echo "t3 body\n"; } }
t3(new D(3)); echo "after t3\n";
function t4($f){ $e = new Exception; $f = null; echo "t4 body\n"; unset($e); echo "t4 unset\n"; }
t4(new D(4)); echo "after t4\n";
--EXPECTF--
t1 body
dtor 1
after t1

Deprecated: Creation of dynamic property D::$x is deprecated in %s on line 5
t2 body
dtor 20
dtor 2
after t2
t3 body
dtor 3
after t3
t4 body
dtor 4
t4 unset
after t4
