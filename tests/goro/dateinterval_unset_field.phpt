--TEST--
unset() of a DateInterval field: get_properties puts it back (at the end once the table was built)
--FILE--
<?php
$i = new DateInterval('P1D');
unset($i->y);
var_dump($i->y, isset($i->y));
var_dump($i);
$i->y = 7;
var_dump($i->y);
unset($i->days, $i->from_string, $i->invert);
var_dump(json_encode($i));
unset($i->m);
var_dump(array_keys((array)$i));
unset($i->days);
var_dump($i);
$d = new DateTime('2024-01-01 UTC');
$d->add($i); echo $d->format('c'), "\n";
$i = DateInterval::createFromDateString('+2 days');
var_dump($i);
unset($i->from_string, $i->date_string);
var_dump($i);
?>
--EXPECT--
int(0)
bool(true)
object(DateInterval)#1 (10) {
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
int(7)
string(87) "{"y":7,"m":0,"d":1,"h":0,"i":0,"s":0,"f":0,"invert":0,"days":false,"from_string":false}"
array(10) {
  [0]=>
  string(1) "y"
  [1]=>
  string(1) "d"
  [2]=>
  string(1) "h"
  [3]=>
  string(1) "i"
  [4]=>
  string(1) "s"
  [5]=>
  string(1) "f"
  [6]=>
  string(6) "invert"
  [7]=>
  string(4) "days"
  [8]=>
  string(11) "from_string"
  [9]=>
  string(1) "m"
}
object(DateInterval)#1 (10) {
  ["y"]=>
  int(7)
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
  ["from_string"]=>
  bool(false)
  ["m"]=>
  int(0)
  ["days"]=>
  bool(false)
}
2031-01-02T00:00:00+00:00
object(DateInterval)#3 (2) {
  ["from_string"]=>
  bool(true)
  ["date_string"]=>
  string(7) "+2 days"
}
object(DateInterval)#3 (2) {
  ["from_string"]=>
  bool(true)
  ["date_string"]=>
  string(7) "+2 days"
}
