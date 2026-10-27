--TEST--
Promoted property errors are reported on the parameter's line
--FILE--
<?php
class Foo {
    public function __construct(
        public int $a,

        public readonly $bar,
    ) {}
}
--EXPECTF--

Fatal error: Readonly property Foo::$bar must have type in %s on line 6
