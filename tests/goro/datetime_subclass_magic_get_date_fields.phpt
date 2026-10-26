--TEST--
A DateTime subclass's __get() is called for the date / timezone fields
--FILE--
<?php
class D extends DateTime {
    public function __get($n) { return "magic:$n"; }
}
$d = new D('2024-01-01', new DateTimeZone('Australia/Melbourne'));
var_dump($d->timezone);
$p = new DatePeriod(new D('2024-01-01'), new DateInterval('P1D'), new D('2024-01-03'));
var_dump($p->getStartDate()->timezone);
$c = clone $d;
var_dump($c->timezone);
var_dump(isset($d->timezone));
var_dump($d);
var_dump($d->timezone);

?>
--EXPECT--
string(14) "magic:timezone"
string(14) "magic:timezone"
string(14) "magic:timezone"
bool(false)
object(D)#1 (3) {
  ["date"]=>
  string(26) "2024-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(19) "Australia/Melbourne"
}
string(14) "magic:timezone"
