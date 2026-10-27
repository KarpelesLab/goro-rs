--TEST--
A finished yield-from delegate's variables are released at the outermost resumer's line
--FILE--
<?php
class Foo {
    public $n;
    function __construct($n) { $this->n = $n; }
    public function __destruct() {
        echo "dtor {$this->n}\n";
        debug_print_backtrace();
    }
}
function foo() {
    $foo = new Foo(1);
    yield;
}
function bar() {
    yield from foo();
}
$gen = bar();
foreach ($gen as $dummy);
echo "--\n";
function baz() {
    $foo = new Foo(2);
    yield 1;
    yield 2;
}
foreach (baz() as $v) {
    echo "v=$v\n";
}
echo "--\n";
$g = baz();
$g->current();
unset($g);
echo "--\n";
function qux() {
    $foo = new Foo(3);
    yield 1;
}
$g = qux();
foreach ($g as $v) {}
echo "end\n";
--EXPECTF--
dtor 1
#0 %s(18): Foo->__destruct()
#1 %s(18): bar()
--
v=1
v=2
dtor 2
#0 %s(25): Foo->__destruct()
--
dtor 2
#0 %s(31): Foo->__destruct()
--
dtor 3
#0 %s(38): Foo->__destruct()
end
