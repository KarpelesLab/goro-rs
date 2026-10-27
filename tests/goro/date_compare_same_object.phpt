--TEST--
Comparing a date object with itself does not call its compare handler
--FILE--
<?php
class D extends DateTime { function __construct() {} }
class Z extends DateTimeZone { function __construct() {} }
$d = new D; $z = new Z; $i = new DateInterval('P1D');
var_dump($d == $d, $d < $d, $d <=> $d, $z == $z, $i == $i, $i <=> $i);
try { var_dump($d == new D); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($z == new Z); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($i == new DateInterval('P1D'));
$u = new DateTimeZone('UTC');
var_dump($u == $u);
try { var_dump($u == new DateTimeZone('+00:00')); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
?>
--EXPECTF--
bool(true)
bool(false)
int(0)
bool(true)
bool(true)
int(0)
DateObjectError: Trying to compare an incomplete DateTime or DateTimeImmutable object
DateObjectError: Trying to compare uninitialized DateTimeZone objects

Warning: Cannot compare DateInterval objects in %s on line 8
bool(false)
bool(true)
DateException: Cannot compare two different kinds of DateTimeZone objects
