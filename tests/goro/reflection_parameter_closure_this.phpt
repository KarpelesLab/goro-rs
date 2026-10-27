--TEST--
new ReflectionParameter() of a closure with $this (by offset or name), of an invokable object; a closure parameter's declaring class is its scope
--FILE--
<?php
namespace Foo;
class T {
    function run() {
        $p = new \ReflectionParameter(function (\stdClass $foo) {}, 0);
        $t = $p->getType();
        var_dump(get_class($t), $t->getName(), $t->isBuiltin(), $p->getDeclaringClass()?->getName(), $p->getDeclaringFunction()->getName());
        $p = new \ReflectionParameter(function ($foo) {}, 0);
        var_dump($p->getType(), $p->getName());
        $p = new \ReflectionParameter(function (string $foo, int $bar) {}, 'bar');
        var_dump($p->getType()->getName(), $p->getPosition());
        $p = new \ReflectionParameter([$this, 'm'], 0);
        var_dump($p->getType()->getName());
        $p = new \ReflectionParameter('Foo\f', 0);
        var_dump($p->getType()->getName());
        $p = new \ReflectionParameter(new I, 0);
        var_dump($p->getType()->getName());
    }
    function m(self $x) {}
}
function f(?\Countable $c) {}
class I { function __invoke(array $a) {} }
(new T)->run();
$p = new \ReflectionParameter(function (\stdClass $foo) {}, 0);
var_dump($p->getType()->getName(), $p->getDeclaringClass());

?>
--EXPECT--
string(19) "ReflectionNamedType"
string(8) "stdClass"
bool(false)
string(5) "Foo\T"
string(24) "{closure:Foo\T::run():5}"
NULL
string(3) "foo"
string(3) "int"
int(1)
string(5) "Foo\T"
string(9) "Countable"
string(5) "array"
string(8) "stdClass"
NULL
