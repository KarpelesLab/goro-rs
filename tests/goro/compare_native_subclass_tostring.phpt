--TEST--
Comparing a string with an extension object whose user subclass declares __toString() (a DateTime subclass) casts it, also inside arrays
--FILE--
<?php
class S { function __toString(): string { return "abc"; } }
var_dump(new S == "abc");
class D extends DateTime { function __toString(): string { return "abc"; } }
var_dump(new D == "abc", "abc" == new D, new D < "abd", new DateTime == "abc");
class X extends ArrayObject { function __toString(): string { return "x"; } }
var_dump(new X == "x");
$s = simplexml_load_string("<a>hi</a>"); var_dump($s == "hi");
var_dump(["a" => new D] == ["a" => "abc"], [new D] == ["abd"], in_array("abc", [new D]), array_search("abc", [1, new D]));

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
int(1)
