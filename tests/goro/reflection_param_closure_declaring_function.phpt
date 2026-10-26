--TEST--
A closure parameter's getDeclaringFunction() answers getClosureThis()/getClosureScopeClass()
--FILE--
<?php
class T {
    function t() {
        $c = function (string $name) { return $this; };
        $r = new ReflectionFunction($c);
        $p = $r->getParameters()[0];
        $d = $p->getDeclaringFunction();
        var_dump(get_class($d), $d->getName(), $d->getClosureThis() === $this, $d->getClosureScopeClass()?->name);
        $p2 = new ReflectionParameter($c, 'name');
        $d2 = $p2->getDeclaringFunction();
        var_dump(get_class($d2), $d2->getClosureThis() === $this);
        $p3 = new ReflectionParameter([$this, 'u'], 0);
        var_dump(get_class($p3->getDeclaringFunction()), $p3->getDeclaringFunction()->getClosureThis());
    }
    function u($x) {}
}
(new T)->t();

?>
--EXPECT--
string(16) "ReflectionMethod"
string(18) "{closure:T::t():4}"
bool(true)
string(1) "T"
string(16) "ReflectionMethod"
bool(true)
string(16) "ReflectionMethod"
NULL
