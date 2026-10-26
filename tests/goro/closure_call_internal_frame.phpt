--TEST--
Closure::call(): the closure is called from the internal Closure->call() frame
--FILE--
<?php
class A { private $x = 1; }
$c = function() { throw new Exception("e"); };
try { $c->call(new A); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
$c2 = function($a) { debug_print_backtrace(); return $this->x; };
var_dump($c2->call(new A, 5));
$c3 = fn() => $this->x;
var_dump($c3->call(new A));
--EXPECTF--
#0 [internal function]: A->{closure:%s:3}()
#1 %s(4): Closure->call(Object(A))
#2 {main}
#0 [internal function]: A->{closure:%s:5}(5)
#1 %s(6): Closure->call(Object(A), 5)
int(1)
int(1)
