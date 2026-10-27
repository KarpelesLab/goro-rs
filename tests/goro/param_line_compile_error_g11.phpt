--TEST--
Compile errors about a parameter are reported on its line (its type's, else its name's) (g11)
--FILE--
<?php
class A {
    public function __construct(
        public
        readonly
        $a,
    ) {}
}
--EXPECTF--

Fatal error: Readonly property A::$a must have type in %s on line 6
