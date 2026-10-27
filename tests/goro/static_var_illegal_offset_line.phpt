--TEST--
Illegal offset type in a static initializer: the statement's line
--FILE--
<?php
function f() {
  static $s = [
    1,
    [] => 2,
  ];
}
--EXPECTF--

Fatal error: Illegal offset type in %s on line 3
