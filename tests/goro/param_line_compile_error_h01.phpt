--TEST--
Compile errors about a parameter are reported on its line (its type's, else its name's) (h01)
--FILE--
<?php
class A {
    public function __construct(
        public
        $a,
        public
        $a,
    ) {}
}
--EXPECTF--

Fatal error: Redefinition of parameter $a in %s on line 7
