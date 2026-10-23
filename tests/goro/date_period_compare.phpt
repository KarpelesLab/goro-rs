--TEST--
DatePeriod == compares the property slots (its fields are not ones)
--FILE--
<?php
$a = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2);
$b = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2);
var_dump($a == $b);
$c = new DatePeriod(new DateTime('2020-01-02'), new DateInterval('P1D'), 2);
var_dump($a == $c, $a != $c);
$d = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P2D'), 2);
var_dump($a == $d);
$e = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 3);
var_dump($a == $e);
foreach ($a as $x) {}
var_dump($a == $b);
var_dump(new DateInterval('P1D') == new DateInterval('P1D'));
var_dump((array) $a == (array) $b);
--EXPECTF--
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)

Warning: Cannot compare DateInterval objects in %s on line 13
bool(false)
bool(false)
