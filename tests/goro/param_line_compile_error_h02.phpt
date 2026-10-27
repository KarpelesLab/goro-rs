--TEST--
Compile errors about a parameter are reported on its line (its type's, else its name's) (h02)
--FILE--
<?php
class A {
    public function __construct(
        private
        public(set)
        int
        $a,
    ) {}
}
--EXPECTF--

Fatal error: Visibility of property A::$a must not be weaker than set visibility in %s on line 6
