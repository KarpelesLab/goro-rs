--TEST--
Deprecated "self" callables passed to a callable parameter: reported at the callee
--FILE--
<?php
class A {
    public function foo() {}
}
class B extends A {
    public function test() {
        $this->hint(
            'self::foo'
        );
        array_map(function (callable $c) {
        }, ['parent::foo']);
    }

    public function hint(callable $c) {}
}
(new B)->test();
echo "done\n";
--EXPECTF--

Deprecated: Use of "self" in callables is deprecated in %s on line 14

Deprecated: Use of "parent" in callables is deprecated in %s on line 10
done
