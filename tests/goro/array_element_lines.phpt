--TEST--
Array literal elements: their warnings and errors on the element's line
--FILE--
<?php
$f = 1.5; $n = null; $o = new stdClass;
$a = [
  $f => 1,

  $n => 2,
  $u,
  ...
  [3],
];
function g() {
    yield 1.5 => 1;
}
try {
    $b = [
        1,
        ...g()];
} catch (Error $e) {
    echo $e->getMessage(), " @ ", $e->getLine(), "\n";
}
try {
    $b = [
        1,

        $o => 2];
} catch (TypeError $e) {
    echo $e->getMessage(), " @ ", $e->getLine(), "\n";
}
--EXPECTF--

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 4

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 6

Warning: Undefined variable $u in %s on line 7
Keys must be of type int|string during array unpacking @ 17
Cannot access offset of type stdClass on array @ 25
