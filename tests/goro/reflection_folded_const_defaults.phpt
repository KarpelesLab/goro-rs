--TEST--
Parameter defaults naming folded constants (PHP_INT_MAX, E_ALL, NAN) stay constants; abstract method defaults are rendered
--FILE--
<?php
const MYC = 3;
interface I { function m($a = PHP_INT_MAX, $b = MYC, $c = I::X, $d = [PHP_EOL], $e = 1 + 2, $f = M_PI); const X = 2; }
abstract class A { abstract function m($a = PHP_INT_MAX, $b = MYC, $c = I::X, $e = 1+2, $f = \PHP_VERSION_ID > 1, $g = new stdClass); }
echo new ReflectionMethod('I', 'm');
echo new ReflectionMethod('A', 'm');
function f($a = PHP_INT_MAX, $b = PHP_EOL, $c = E_ALL, $d = \PHP_INT_SIZE, $e = M_PI, $f = null, $g = NAN, $h = true) {}
foreach ([new ReflectionMethod('I','m'), new ReflectionFunction('f')] as $fn) {
  foreach ($fn->getParameters() as $p) {
    var_dump($p->isDefaultValueConstant() ? $p->getDefaultValueConstantName() : $p->getDefaultValue());
  }
}
class P { function m($e = PHP_INT_MAX, $f = E_ALL) {} }
eval("class Q extends P { function m() {} }");
--EXPECTF--
Method [ <user> abstract public method m ] {
  @@ %s 3 - 3

  - Parameters [6] {
    Parameter #0 [ <optional> $a = PHP_INT_MAX ]
    Parameter #1 [ <optional> $b = MYC ]
    Parameter #2 [ <optional> $c = I::X ]
    Parameter #3 [ <optional> $d = [PHP_EOL] ]
    Parameter #4 [ <optional> $e = 3 ]
    Parameter #5 [ <optional> $f = M_PI ]
  }
}
Method [ <user> abstract public method m ] {
  @@ %s 4 - 4

  - Parameters [6] {
    Parameter #0 [ <optional> $a = PHP_INT_MAX ]
    Parameter #1 [ <optional> $b = MYC ]
    Parameter #2 [ <optional> $c = I::X ]
    Parameter #3 [ <optional> $e = 3 ]
    Parameter #4 [ <optional> $f = PHP_VERSION_ID > 1 ]
    Parameter #5 [ <optional> $g = new \stdClass() ]
  }
}
string(11) "PHP_INT_MAX"
string(3) "MYC"
string(4) "I::X"
array(1) {
  [0]=>
  string(1) "
"
}
int(3)
string(4) "M_PI"
string(11) "PHP_INT_MAX"
string(7) "PHP_EOL"
string(5) "E_ALL"
string(12) "PHP_INT_SIZE"
string(4) "M_PI"
NULL
string(3) "NAN"
bool(true)

Fatal error: Declaration of Q::m() must be compatible with P::m($e = PHP_INT_MAX, $f = E_ALL) in %s : eval()'d code on line 1
Stack trace:
#0 {main}
