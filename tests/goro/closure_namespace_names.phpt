--TEST--
Closures and arrow functions resolve names in the namespace and imports they are declared in
--FILE--
<?php
namespace N;
use ArrayObject as AO;
use function strtoupper as up;
class C {}
const K = 1;
function bar() { return "bar"; }
function strlen($x) { return "mine"; }
$f = function () {
    var_dump(new C, K, __NAMESPACE__, C::class, bar(), strlen("x"), up("a"), AO::class);
};
$f();
$g = fn() => bar() . strlen("x");
var_dump($g());
function t() { return (function () { return bar(); })(); }
var_dump(t());
?>
--EXPECT--
object(N\C)#2 (0) {
}
int(1)
string(1) "N"
string(3) "N\C"
string(3) "bar"
string(4) "mine"
string(1) "A"
string(11) "ArrayObject"
string(7) "barmine"
string(3) "bar"
