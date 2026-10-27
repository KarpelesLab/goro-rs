--TEST--
Compile errors about a parameter are reported on its line (its type's, else its name's) (h09)
--FILE--
<?php
class A {
    public function __construct(
        public
        $this
    ) {}
}
--EXPECTF--

Fatal error: Cannot use $this as parameter in %s on line 5
