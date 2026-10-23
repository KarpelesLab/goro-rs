--TEST--
Restored extra properties come before the date fields
--FILE--
<?php
var_dump(unserialize('O:8:"DateTime":4:{s:4:"date";s:26:"2024-01-15 10:30:45.000000";s:13:"timezone_type";i:3;s:8:"timezone";s:3:"UTC";s:3:"foo";i:1;}'));
var_dump(unserialize('O:17:"DateTimeImmutable":4:{s:3:"bar";i:2;s:4:"date";s:26:"2024-01-15 10:30:45.000000";s:13:"timezone_type";i:3;s:8:"timezone";s:3:"UTC";}'));
var_dump(unserialize('O:12:"DateTimeZone":3:{s:13:"timezone_type";i:3;s:8:"timezone";s:3:"UTC";s:3:"foo";i:1;}'));
$d = new DateTime('2024-01-01'); $d->x = 5; var_dump($d);
$z = new DateTimeZone('UTC'); $z->x = 5; var_dump($z);
class D extends DateTime { public $p = 1; }
var_dump(unserialize(serialize(new D('2024-01-01'))));
$i = new DateInterval('P1D'); $i->x=1; var_dump(unserialize(serialize($i)));
var_dump(DateTime::__set_state(['foo'=>1,'date'=>'2024-01-15 10:30:45.000000','timezone_type'=>3,'timezone'=>'UTC']));
var_dump(DateTimeZone::__set_state(['foo'=>1,'timezone_type'=>3,'timezone'=>'UTC']));
$d = new DateTime('2024-01-01'); $d->__unserialize(['date'=>'2024-01-15 10:30:45.000000','timezone_type'=>3,'timezone'=>'UTC','q'=>1]); var_dump($d);
--EXPECTF--

Deprecated: Creation of dynamic property DateTime::$foo is deprecated in %s on line 2
object(DateTime)#1 (4) {
  ["foo"]=>
  int(1)
  ["date"]=>
  string(26) "2024-01-15 10:30:45.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateTimeImmutable::$bar is deprecated in %s on line 3
object(DateTimeImmutable)#1 (4) {
  ["bar"]=>
  int(2)
  ["date"]=>
  string(26) "2024-01-15 10:30:45.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateTimeZone::$foo is deprecated in %s on line 4
object(DateTimeZone)#1 (3) {
  ["foo"]=>
  int(1)
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateTime::$x is deprecated in %s on line 5
object(DateTime)#1 (4) {
  ["x"]=>
  int(5)
  ["date"]=>
  string(26) "2024-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateTimeZone::$x is deprecated in %s on line 6
object(DateTimeZone)#2 (3) {
  ["x"]=>
  int(5)
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
object(D)#3 (4) {
  ["p"]=>
  int(1)
  ["date"]=>
  string(26) "2024-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateInterval::$x is deprecated in %s on line 9

Deprecated: Creation of dynamic property DateInterval::$x is deprecated in %s on line 9
object(DateInterval)#4 (11) {
  ["x"]=>
  int(1)
  ["y"]=>
  int(0)
  ["m"]=>
  int(0)
  ["d"]=>
  int(1)
  ["h"]=>
  int(0)
  ["i"]=>
  int(0)
  ["s"]=>
  int(0)
  ["f"]=>
  float(0)
  ["invert"]=>
  int(0)
  ["days"]=>
  bool(false)
  ["from_string"]=>
  bool(false)
}
object(DateTime)#4 (3) {
  ["date"]=>
  string(26) "2024-01-15 10:30:45.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
object(DateTimeZone)#4 (2) {
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateTime::$q is deprecated in %s on line 12
object(DateTime)#4 (4) {
  ["q"]=>
  int(1)
  ["date"]=>
  string(26) "2024-01-15 10:30:45.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
