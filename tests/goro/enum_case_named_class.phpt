--TEST--
An enum case cannot be called "class"
--FILE--
<?php
enum E {
  case A;
  case class;
}
--EXPECTF--

Fatal error: A class constant must not be called 'class'; it is reserved for class name fetching in %s on line 4
