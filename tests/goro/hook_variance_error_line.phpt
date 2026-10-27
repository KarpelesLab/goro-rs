--TEST--
Incompatible hook declaration: on the child hook's line
--FILE--
<?php
class A {
    public int $a { get => 1; }
}
class B extends A {
    public int|float $a {

        get => 2.5;
    }
}
--EXPECTF--

Fatal error: Declaration of B::$a::get(): int|float must be compatible with A::$a::get(): int in %s on line 8
