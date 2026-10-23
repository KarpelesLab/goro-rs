--TEST--
DateInterval property writes are coerced like date_interval_write_property() (days is kept)
--FILE--
<?php
$i = new DateInterval('P1Y2M3DT4H5M6S');
$i->y = "5"; $i->m = 7.9; $i->d = true; $i->h = null; $i->i = "12abc"; $i->f = "0.5"; $i->invert = 1;
var_dump($i);
$i->days = 10;
var_dump($i->days);
$i->s = [];
var_dump($i->s);
echo $i->format('%y %m %d %h %i %s %f %R %a'), "\n";
$i->foo = 1;
var_dump($i->foo);
--EXPECTF--
object(DateInterval)#1 (10) {
  ["y"]=>
  int(5)
  ["m"]=>
  int(7)
  ["d"]=>
  int(1)
  ["h"]=>
  int(0)
  ["i"]=>
  int(12)
  ["s"]=>
  int(6)
  ["f"]=>
  float(0.5)
  ["invert"]=>
  int(1)
  ["days"]=>
  bool(false)
  ["from_string"]=>
  bool(false)
}
bool(false)
int(0)
5 7 1 0 12 0 500000 - (unknown)

Deprecated: Creation of dynamic property DateInterval::$foo is deprecated in %s on line 10
int(1)
