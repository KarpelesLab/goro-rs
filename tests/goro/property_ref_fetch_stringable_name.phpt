--TEST--
$x = &$o->{$stringable}: the property name is the object's __toString()
--FILE--
<?php
class D extends DateTime { function __toString(): string { return $this->format("Y-m-d H:i:s"); } }
$d = new D('2014-02-05');
$row = ['date' => $d];
$res = new stdClass;
$x = &$res;
$x = &$x->{$row['date']};
$x = $row['date'];
var_dump($res);
$res2 = [];
$x2 = &$res2;
$x2 = &$x2[(string) $row['date']];
var_dump($res2);
$o = new stdClass;
$o->{$d} = 1;
var_dump($o);
var_dump(property_exists($o, '2014-02-05 00:00:00'));

?>
--EXPECT--
object(stdClass)#2 (1) {
  ["2014-02-05 00:00:00"]=>
  &object(D)#1 (3) {
    ["date"]=>
    string(26) "2014-02-05 00:00:00.000000"
    ["timezone_type"]=>
    int(3)
    ["timezone"]=>
    string(3) "UTC"
  }
}
array(1) {
  ["2014-02-05 00:00:00"]=>
  &NULL
}
object(stdClass)#3 (1) {
  ["2014-02-05 00:00:00"]=>
  int(1)
}
bool(true)
