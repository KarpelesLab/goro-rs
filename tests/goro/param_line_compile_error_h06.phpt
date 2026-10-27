--TEST--
Compile errors about a parameter are reported on its line (its type's, else its name's) (h06)
--FILE--
<?php
class A {
    public function __construct(
        public
        $a
        = 5,
        public
        int
        $b
        = null,
    ) {}
}
--EXPECTF--

Fatal error: Cannot use null as default value for parameter $b of type int in %s on line 8
