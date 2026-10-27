--TEST--
Errors of a class's attributes are reported on the class keyword's line
--FILE--
<?php
echo "a\n";

#[Attribute]
#[Foo(...[1])]
class
C1 {}
--EXPECTF--

Fatal error: Cannot use unpacking in attribute argument list in %s on line 6
