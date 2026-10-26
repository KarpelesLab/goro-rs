--TEST--
(array) and settype() of a Generator give an empty array
--FILE--
<?php
function g() { yield 1; }
$g = g(); var_dump((array) $g); settype($g, "array"); var_dump($g);
--EXPECTF--
array(0) {
}
array(0) {
}
