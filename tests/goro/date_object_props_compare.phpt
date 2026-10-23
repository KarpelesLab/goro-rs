--TEST--
DateInterval comparisons and max()/min(); get_object_vars()/foreach of DateTime and DateTimeZone show no date fields
--FILE--
<?php
$a = new DateInterval('P1D'); $b = new DateInterval('P2D');
var_dump($a <=> $b, $a >= $b, $a < $b, $a == $b, max($a, $b) === $a, min($a, $b) === $a);
$i = new DateInterval('P1D');
var_dump(get_object_vars(new DateTimeZone('UTC')), get_object_vars(new DateTime('2024-01-01')));
foreach (new DateTime('2024-01-01') as $k => $v) echo $k, "\n";
foreach (new DateTimeZone('Europe/Paris') as $k => $v) echo $k, "\n";
var_dump(count((array) new DateTime('2024-01-01')), json_encode(new DateTimeZone('UTC')));
class MyDT extends DateTime { public $x = 1; }
var_dump(get_object_vars(new MyDT('2024-01-01')));
foreach (new MyDT('2024-01-01') as $k => $v) echo $k, "\n";
var_dump(new DateTime('2024-01-01') == new DateTime('2024-01-01'), new DateTime('2024-01-01') < new DateTime('2024-01-02'));
--EXPECTF--

Warning: Cannot compare DateInterval objects in %s on line 3

Warning: Cannot compare DateInterval objects in %s on line 3

Warning: Cannot compare DateInterval objects in %s on line 3

Warning: Cannot compare DateInterval objects in %s on line 3

Warning: Cannot compare DateInterval objects in %s on line 3

Warning: Cannot compare DateInterval objects in %s on line 3
int(1)
bool(false)
bool(false)
bool(false)
bool(true)
bool(false)
array(0) {
}
array(0) {
}
int(3)
string(36) "{"timezone_type":3,"timezone":"UTC"}"
array(1) {
  ["x"]=>
  int(1)
}
x
bool(true)
bool(true)
