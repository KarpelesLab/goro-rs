--TEST--
in_array()/array_search(): loose comparison casts Stringable objects
--FILE--
<?php
// in_array()/array_search() compare like ==: Stringable objects meeting
// strings are cast with __toString()
class S { function __construct(private $v) {} function __toString(): string { return $this->v; } }
var_dump(in_array("a", [new S("a")]), in_array(new S("b"), ["a", "b"]), in_array("c", [new S("a")]));
var_dump(array_search("y", ["x", new S("y")]), array_search(new S("x"), ["q" => "x"]));
var_dump(in_array("a", [new S("a")], true), in_array(1, [new stdClass]));
--EXPECTF--
bool(true)
bool(true)
bool(false)
int(1)
string(1) "q"

Notice: Object of class stdClass could not be converted to int in %s on line 7
bool(false)
bool(true)
