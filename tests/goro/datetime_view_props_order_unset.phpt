--TEST--
DateTime view fields: the script's own properties named like them in get_object_vars()/foreach, var_dump() order, unset()
--FILE--
<?php
$p = new DateTime('2024-01-01 10:00:00', new DateTimeZone('Europe/Paris'));
var_dump(isset($p->timezone), property_exists($p, 'timezone'));
$p->timezone = 'abc';
var_dump($p->timezone, isset($p->timezone), property_exists($p, 'timezone'));
var_dump(get_object_vars($p));
foreach ($p as $k => $v) echo "$k => $v\n";
var_dump($p);
var_dump($p->timezone);
$p->modify('+1 day');
var_dump($p->timezone);
var_dump((array) $p);
$p->date = 'zz';
var_dump($p->date, $p->format('c'));
unset($p->timezone);
var_dump(isset($p->timezone));
var_dump($p);
$c = clone $p;
var_dump($c->date);
echo serialize($p), "\n";
$z = new DateTimeZone('UTC');
$z->timezone = 'q';
var_dump($z->timezone, $z);

?>
--EXPECTF--
bool(false)
bool(false)

Deprecated: Creation of dynamic property DateTime::$timezone is deprecated in %s on line 4
string(3) "abc"
bool(true)
bool(true)
array(1) {
  ["timezone"]=>
  string(3) "abc"
}
timezone => abc
object(DateTime)#1 (3) {
  ["timezone"]=>
  string(12) "Europe/Paris"
  ["date"]=>
  string(26) "2024-01-01 10:00:00.000000"
  ["timezone_type"]=>
  int(3)
}
string(3) "abc"
string(3) "abc"
array(3) {
  ["timezone"]=>
  string(12) "Europe/Paris"
  ["date"]=>
  string(26) "2024-01-02 10:00:00.000000"
  ["timezone_type"]=>
  int(3)
}

Deprecated: Creation of dynamic property DateTime::$date is deprecated in %s on line 13
string(2) "zz"
string(25) "2024-01-02T10:00:00+01:00"
bool(false)
object(DateTime)#1 (3) {
  ["date"]=>
  string(26) "2024-01-02 10:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(12) "Europe/Paris"
}
string(2) "zz"
O:8:"DateTime":3:{s:4:"date";s:26:"2024-01-02 10:00:00.000000";s:13:"timezone_type";i:3;s:8:"timezone";s:12:"Europe/Paris";}

Deprecated: Creation of dynamic property DateTimeZone::$timezone is deprecated in %s on line 22
string(1) "q"
object(DateTimeZone)#3 (2) {
  ["timezone"]=>
  string(3) "UTC"
  ["timezone_type"]=>
  int(3)
}
