--TEST--
BcMath\Number methods: zpp of all arguments before the number is parsed; pow() result scale check
--FILE--
<?php
$n = new BcMath\Number("1.5");
foreach ([fn() => $n->add("x", []), fn() => $n->pow(PHP_INT_MAX), fn() => $n->pow(2), fn() => $n->powmod("a", -1), fn() => $n->compare("z", "q"), fn() => $n->divmod("1", []), fn() => $n->__toString(1), fn() => (string)$n] as $f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
--EXPECT--
TypeError: BcMath\Number::add(): Argument #2 ($scale) must be of type ?int, array given
ValueError: scale of the result is too large
object(BcMath\Number)#10 (2) {
  ["value"]=>
  string(4) "2.25"
  ["scale"]=>
  int(2)
}
ValueError: BcMath\Number::powmod(): Argument #1 ($exponent) is not well-formed
TypeError: BcMath\Number::compare(): Argument #2 ($scale) must be of type ?int, string given
TypeError: BcMath\Number::divmod(): Argument #2 ($scale) must be of type ?int, array given
ArgumentCountError: BcMath\Number::__toString() expects exactly 0 arguments, 1 given
string(3) "1.5"
