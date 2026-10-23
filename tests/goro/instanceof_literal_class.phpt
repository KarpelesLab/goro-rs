--TEST--
instanceof a literal class: non-objects, the class itself, parents, interfaces, aliases, self/static/parent
--FILE--
<?php
interface I {}
class A implements I {}
class B extends A {}
class_alias('A', 'AliasA');
function gen() { yield 1; }
$vals = [null, 1, 'A', [1], new A, new B, new stdClass, function () {}, gen()];
foreach ($vals as $i => $v) {
    echo $i, ': ', json_encode([
        $v instanceof A, $v instanceof B, $v instanceof I, $v instanceof AliasA,
        $v instanceof stdClass, $v instanceof Closure, $v instanceof Generator,
        $v instanceof Traversable, $v instanceof Undefined_Class,
    ]), "\n";
}
class C {
    function t($o) { return [$o instanceof self, $o instanceof static, $o instanceof C]; }
}
class D extends C {
    function u($o) { return [$o instanceof parent, $o instanceof self]; }
}
echo json_encode((new C)->t(new C)), json_encode((new D)->t(new C)), json_encode((new D)->u(new D)), "\n";
$cls = 'A';
echo json_encode([new B instanceof $cls, new B instanceof $vals[4]]), "\n";
echo json_encode([@$undef instanceof A]), "\n";
echo json_encode([$undef2 instanceof A]), "\n";
function f() { return (new A) instanceof A; }
echo json_encode([f(), f()]), "\n";
spl_autoload_register(function ($c) { echo "autoload $c\n"; });
echo json_encode([null instanceof NotLoaded, (new A) instanceof NotLoaded2]), "\n";
?>
--EXPECTF--
0: [false,false,false,false,false,false,false,false,false]
1: [false,false,false,false,false,false,false,false,false]
2: [false,false,false,false,false,false,false,false,false]
3: [false,false,false,false,false,false,false,false,false]
4: [true,false,true,true,false,false,false,false,false]
5: [true,true,true,true,false,false,false,false,false]
6: [false,false,false,false,true,false,false,false,false]
7: [false,false,false,false,false,true,false,false,false]
8: [false,false,false,false,false,false,true,true,false]
[true,true,true][true,false,true][true,true]
[true,true]
[false]

Warning: Undefined variable $undef2 in %s on line 25
[false]
[true,true]
[false,false]
