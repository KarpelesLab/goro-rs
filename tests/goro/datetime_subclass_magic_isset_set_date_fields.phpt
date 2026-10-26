--TEST--
A DateTime subclass's __isset() / __set() / __unset() handle the date fields
--FILE--
<?php
class D extends DateTime {
    public function __get($n) { return "magic:$n"; }
    public function __isset($n) { echo "isset($n)\n"; return true; }
    public function __set($n, $v) { echo "set($n, $v)\n"; }
    public function __unset($n) { echo "unset($n)\n"; }
}
$d = new D('2024-01-01');
var_dump(isset($d->timezone), isset($d->date), empty($d->timezone_type));
$d->timezone = 'Europe/Paris';
$d->date = 'x';
unset($d->timezone);
var_dump($d->timezone ?? 'none');
var_dump($d);
$p = new DateTime('2024-01-01');

?>
--EXPECT--
isset(timezone)
isset(date)
isset(timezone_type)
bool(true)
bool(true)
bool(false)
set(timezone, Europe/Paris)
set(date, x)
unset(timezone)
isset(timezone)
string(14) "magic:timezone"
object(D)#1 (3) {
  ["date"]=>
  string(26) "2024-01-01 00:00:00.000000"
  ["timezone_type"]=>
  int(3)
  ["timezone"]=>
  string(3) "UTC"
}
