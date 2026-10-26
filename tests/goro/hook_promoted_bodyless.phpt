--TEST--
A promoted property hook needs a body
--FILE--
<?php
class A {
    public function __construct(
        public $a,
        public $x {
            get;
        }
    ) {}
}
--EXPECTF--

Fatal error: Non-abstract property hook must have a body in %s on line 6
