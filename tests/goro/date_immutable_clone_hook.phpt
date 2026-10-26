--TEST--
DateTimeImmutable methods clone through __clone (uninitialized while it runs)
--FILE--
<?php
#[AllowDynamicProperties]
class I extends DateTimeImmutable { public $n = 0; private function __clone() { $this->n++; $this->extra = 1; } }
$i = new I('2024-01-01');
$j = $i->modify('+1 day');
var_dump($j);
$k = $j->add(new DateInterval('P1D'))->setTime(1, 2);
var_dump($k, $k->n);
class J extends DateTimeImmutable { function __clone() { try { $this->format('Y'); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } } }
echo (new J('2020-01-01'))->modify('+1 day')->format('Y-m-d'), "\n";
--EXPECT--
object(I)#2 (5) {
  ["n"]=>
  int(1)
  ["extra"]=>
  int(1)
  ["date"]=>
  string(26) "2024-01-02 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
object(I)#3 (5) {
  ["n"]=>
  int(3)
  ["extra"]=>
  int(1)
  ["date"]=>
  string(26) "2024-01-03 01:02:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
int(3)
DateObjectError: Object of type J (inheriting DateTimeImmutable) has not been correctly initialized by calling parent::__construct() in its constructor
2020-01-02
