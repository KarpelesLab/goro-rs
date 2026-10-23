--TEST--
A property typed parent in a class without a parent
--FILE--
<?php
class A { public ?parent $p; }
--EXPECTF--
Fatal error: Cannot use "parent" when current class scope has no parent in %s on line %d
