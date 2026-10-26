--TEST--
Static variables of closures declared in a class method are per closure object
--FILE--
<?php
class Q {
    function a() { return function () { static $n = 0; return ++$n; }; }
    function b() { return static function () { static $n = 0; return ++$n; }; }
    function c($v) { return function () use ($v) { static $n = 0; return ++$n; }; }
    function d() { return function () { static $n = 0; $this; return ++$n; }; }
}
$q = new Q;
foreach (['a', 'b', 'd'] as $m) { $x = $q->$m(); $y = $q->$m(); echo $m, ": ", $x(), $x(), $y(), $x(), "\n"; }
$x = $q->c(1); $y = $q->c(2); echo "c: ", $x(), $x(), $y(), $x(), "\n";

?>
--EXPECT--
a: 1213
b: 1213
d: 1213
c: 1213
