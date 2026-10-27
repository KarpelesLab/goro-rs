--TEST--
Illegal offset type in a parameter default: the array's line
--FILE--
<?php
function g($x = [
  1,
  [] => 2,
]) {}
--EXPECTF--

Fatal error: Illegal offset type in %s on line 2
