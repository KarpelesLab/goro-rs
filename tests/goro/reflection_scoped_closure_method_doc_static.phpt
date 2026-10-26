--TEST--
The ReflectionMethod of a class-scoped closure has its doc comment, static flag and parameter defaults
--FILE--
<?php
class T {
    function t() {
        $c = /** @param list<int> $a */ static function (array $a = [], int ...$r): int { return 0; };
        $d = (new ReflectionFunction($c))->getParameters()[0]->getDeclaringFunction();
        var_dump($d->getDocComment(), $d->getNumberOfParameters(), $d->isStatic(), $d->getReturnType()?->getName(), $d->isVariadic());
        var_dump($d->getStartLine(), $d->getFileName() === __FILE__, $d->getShortName());
        var_dump(count($d->getParameters()), $d->getParameters()[0]->getName(), $d->getParameters()[0]->isDefaultValueAvailable());
    }
}
(new T)->t();

?>
--EXPECT--
string(26) "/** @param list<int> $a */"
int(2)
bool(true)
string(3) "int"
bool(true)
int(4)
bool(true)
string(18) "{closure:T::t():4}"
int(2)
string(1) "a"
bool(true)
