--TEST--
iterator_to_array(DatePeriod): one iterator handle (get_iterator), object ids
--FILE--
<?php
$p = new DatePeriod(new DateTimeImmutable('2024-01-01'), new DateInterval('P1D'), 2);
$x = new stdClass; $y = new stdClass; $z = new stdClass;
var_dump(spl_object_id($x), spl_object_id($z));
unset($x, $y, $z);
$a = iterator_to_array($p);
foreach ($a as $v) var_dump(spl_object_id($v));
unset($a, $v);
var_dump(spl_object_id(new stdClass));
--EXPECT--
int(3)
int(4)
int(2)
int(3)
int(5)
int(5)
