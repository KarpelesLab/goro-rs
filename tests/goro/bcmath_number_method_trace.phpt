--TEST--
BcMath\Number methods throw from their own frame (no dispatcher frame)
--FILE--
<?php
// BcMath\Number methods throw from the internal method's own frame
function f($n) { return $n->sqrt(); }
try { f(new BcMath\Number("-4")); } catch (ValueError $e) { echo $e->getTraceAsString(), "\n"; }
try { (new BcMath\Number("1"))->div(0); } catch (DivisionByZeroError $e) { echo $e->getTraceAsString(), "\n"; }
try { (new BcMath\Number("1"))->add(); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { new BcMath\Number("abc"); } catch (ValueError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
function g() { var_dump(count(debug_backtrace())); }
$n = new BcMath\Number(2);
echo $n->pow(3), "\n";
--EXPECTF--
#0 %s(3): BcMath\Number->sqrt()
#1 %s(4): f(Object(BcMath\Number))
#2 {main}
#0 %s(5): BcMath\Number->div(0)
#1 {main}
BcMath\Number::add() expects at least 1 argument, 0 given
#0 %s(6): BcMath\Number->add()
#1 {main}
BcMath\Number::__construct(): Argument #1 ($num) is not well-formed
#0 %s(7): BcMath\Number->__construct('abc')
#1 {main}
8
