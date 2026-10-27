--TEST--
DateTime view fields overlaid on the script's own properties: casts, serialize, clone, unset, setTimezone()
--FILE--
<?php
$d = new DateTime('2024-01-01 10:00:00', new DateTimeZone('Europe/Paris'));
$d->foo = 1;
$d->timezone = 'mine';
$d->bar = 2;
var_dump($d);
var_dump(get_object_vars($d));
foreach ($d as $k => $v) echo "$k => ", var_export($v, true), "\n";
print_r($d); echo "\n";
var_dump((array) $d);
echo json_encode($d), "\n";
var_export($d); echo "\n";
echo serialize($d), "\n";
$c = clone $d;
var_dump($c->timezone, get_object_vars($c));
unset($c->timezone);
var_dump($c, get_object_vars($c));
$d->setTimezone(new DateTimeZone('+05:00'));
var_dump($d);
unset($d->timezone, $d->foo);
var_dump($d);
$i = new DateTimeImmutable('2024-01-01 10:00:00 UTC');
$i->timezone_type = 'x';
$j = $i->modify('+1 day');
var_dump($j, get_object_vars($j));
$e = new DateTime('2024-01-01');
unset($e->date);
var_dump($e);
$z = new DateTimeZone('Europe/Paris');
$z->timezone_type = 9;
var_dump($z, get_object_vars($z));
unset($z->timezone_type);
var_dump($z);

?>
--EXPECTF--

Deprecated: Creation of dynamic property DateTime::$foo is deprecated in %s on line 3

Deprecated: Creation of dynamic property DateTime::$timezone is deprecated in %s on line 4

Deprecated: Creation of dynamic property DateTime::$bar is deprecated in %s on line 5
object(DateTime)#1 (5) {
  ["foo"]=>
  int(1)
  ["timezone"]=>
  string(12) "Europe/Paris"
  ["bar"]=>
  int(2)
  ["date"]=>
  string(26) "2024-01-01 10:00:00.000000"
  ["timezone_type"]=>
  int(3)
}
array(3) {
  ["foo"]=>
  int(1)
  ["timezone"]=>
  string(4) "mine"
  ["bar"]=>
  int(2)
}
foo => 1
timezone => 'mine'
bar => 2
DateTime Object
(
    [foo] => 1
    [timezone] => Europe/Paris
    [bar] => 2
    [date] => 2024-01-01 10:00:00.000000
    [timezone_type] => 3
)

array(5) {
  ["foo"]=>
  int(1)
  ["timezone"]=>
  string(12) "Europe/Paris"
  ["bar"]=>
  int(2)
  ["date"]=>
  string(26) "2024-01-01 10:00:00.000000"
  ["timezone_type"]=>
  int(3)
}
{"foo":1,"timezone":"Europe\/Paris","bar":2,"date":"2024-01-01 10:00:00.000000","timezone_type":3}
\DateTime::__set_state(array(
   'foo' => 1,
   'timezone' => 'Europe/Paris',
   'bar' => 2,
   'date' => '2024-01-01 10:00:00.000000',
   'timezone_type' => 3,
))
O:8:"DateTime":5:{s:4:"date";s:26:"2024-01-01 10:00:00.000000";s:13:"timezone_type";i:3;s:8:"timezone";s:12:"Europe/Paris";s:3:"foo";i:1;s:3:"bar";i:2;}
string(4) "mine"
array(3) {
  ["foo"]=>
  int(1)
  ["timezone"]=>
  string(4) "mine"
  ["bar"]=>
  int(2)
}
object(DateTime)#2 (5) {
  ["foo"]=>
  int(1)
  ["bar"]=>
  int(2)
  ["date"]=>
  string(26) "2024-01-01 10:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(12) "Europe/Paris"
}
array(2) {
  ["foo"]=>
  int(1)
  ["bar"]=>
  int(2)
}
object(DateTime)#1 (5) {
  ["foo"]=>
  int(1)
  ["timezone"]=>
  string(6) "+05:00"
  ["bar"]=>
  int(2)
  ["date"]=>
  string(26) "2024-01-01 14:00:00.000000"
  ["timezone_type"]=>
  int(1)
}
object(DateTime)#1 (4) {
  ["bar"]=>
  int(2)
  ["date"]=>
  string(26) "2024-01-01 14:00:00.000000"
  ["timezone_type"]=>
  int(1)
  ["timezone"]=>
  string(6) "+05:00"
}

Deprecated: Creation of dynamic property DateTimeImmutable::$timezone_type is deprecated in %s on line 23
object(DateTimeImmutable)#4 (3) {
  ["timezone_type"]=>
  int(3)
  ["date"]=>
  string(26) "2024-01-02 10:00:00.000000"
  ["timezone"]=>
  string(3) "UTC"
}
array(1) {
  ["timezone_type"]=>
  string(1) "x"
}
object(DateTime)#5 (3) {
  ["date"]=>
  string(26) "2024-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}

Deprecated: Creation of dynamic property DateTimeZone::$timezone_type is deprecated in %s on line 30
object(DateTimeZone)#6 (2) {
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(12) "Europe/Paris"
}
array(1) {
  ["timezone_type"]=>
  int(9)
}
object(DateTimeZone)#6 (2) {
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(12) "Europe/Paris"
}
