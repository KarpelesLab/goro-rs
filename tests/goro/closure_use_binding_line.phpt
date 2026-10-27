--TEST--
A closure's use() binding is on the variable's line
--FILE--
<?php
$f = function()
  use
  ($undef) { return $undef; };
var_dump($f());
--EXPECTF--

Warning: Undefined variable $undef in %s on line 4
NULL
