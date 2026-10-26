--TEST--
serialize(): an array among __sleep()'s names warns "Array to string conversion"
--FILE--
<?php
class D { function __sleep() { return [1, []]; } }
var_dump(serialize(new D));
--EXPECTF--

Warning: serialize(): D::__sleep() should return an array only containing the names of instance-variables to serialize in %s on line %d

Warning: serialize(): "1" returned as member variable from __sleep() but does not exist in %s on line %d

Warning: serialize(): D::__sleep() should return an array only containing the names of instance-variables to serialize in %s on line %d

Warning: Array to string conversion in %s on line %d

Warning: serialize(): "Array" returned as member variable from __sleep() but does not exist in %s on line %d
string(12) "O:1:"D":0:{}"
