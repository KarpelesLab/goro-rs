--TEST--
new (CONST) instantiates the class named by the constant's value
--FILE--
<?php
namespace A { class X { } }
namespace B {
const C = "A\\X";
define("RF", "A\\X");
var_dump(new (RF), new (C), new ("A\\" . "X"));
$f = fn() => "A\\X"; var_dump(new ($f()));
}

?>
--EXPECT--
object(A\X)#1 (0) {
}
object(A\X)#2 (0) {
}
object(A\X)#3 (0) {
}
object(A\X)#2 (0) {
}
