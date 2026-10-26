--TEST--
=== frees its temporary object operands
--FILE--
<?php
// IS_IDENTICAL frees its temporary object operands
function f() { return new stdClass; }
var_dump(f() === f(), spl_object_id(new stdClass));
var_dump(f() !== null, spl_object_id(new stdClass));
var_dump(null === f(), spl_object_id(new stdClass));
class D { function __destruct() { echo "dtor\n"; } }
function g() { return new D; }
var_dump(g() === null);
echo "after\n";
--EXPECT--
bool(false)
int(2)
bool(true)
int(2)
bool(false)
int(2)
dtor
bool(false)
after
