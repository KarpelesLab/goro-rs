--TEST--
Illegal offset type in a constant declaration: the constant's line
--FILE--
<?php
const
  A = [
  1,
  [] => 2,
];
--EXPECTF--

Fatal error: Illegal offset type in %s on line 3
